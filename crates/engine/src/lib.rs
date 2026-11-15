pub mod poker;
pub mod replay;
pub mod tournament;
use poker::{best, evaluate, valid_cards};
use rand::{SeedableRng, seq::SliceRandom};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const RULES_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Pricing {
    FirstPrice,
    #[default]
    SecondPrice,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    pub seats: usize,
    pub auction_ms: u64,
    pub pricing: Pricing,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            seats: 6,
            auction_ms: 1000,
            pricing: Pricing::SecondPrice,
        }
    }
}
impl Rules {
    pub fn validate(&self) -> Result<(), GameError> {
        if !(2..=8).contains(&self.seats) || !(250..=10000).contains(&self.auction_ms) {
            Err(GameError::InvalidRules)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub house: bool,
    pub balance: u32,
    pub score: u32,
    pub cards: Vec<u8>,
}
impl Player {
    pub fn new(id: String, name: String, house: bool) -> Self {
        Self {
            id,
            name,
            house,
            balance: 100,
            score: 0,
            cards: Vec::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub round: usize,
    pub kind: String,
    pub data: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub rules_version: String,
    pub rules: Rules,
    pub round: usize,
    pub card: Option<u8>,
    pub players: Vec<Player>,
    pub pot: u32,
    pub finished: bool,
    pub commitment: String,
    pub seed: Option<String>,
    pub seq: u64,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum GameError {
    #[error("invalid_rules")]
    InvalidRules,
    #[error("match_finished")]
    Finished,
    #[error("stale_round")]
    StaleRound,
    #[error("not_seated")]
    NotSeated,
    #[error("already_bid")]
    AlreadyBid,
    #[error("insufficient_balance")]
    InsufficientBalance,
    #[error("inventory_full")]
    InventoryFull,
    #[error("invalid_cards")]
    InvalidCards,
}

/// Private state intentionally does not implement Serialize.
#[derive(Clone)]
pub struct Game {
    pub rules: Rules,
    pub players: Vec<Player>,
    pub round: usize,
    pub pot: u32,
    seed: [u8; 32],
    deck: Vec<u8>,
    bids: Vec<Option<u32>>,
    finished: bool,
    pub events: Vec<Event>,
}
impl Game {
    pub fn new(rules: Rules, players: Vec<Player>, seed: [u8; 32]) -> Result<Self, GameError> {
        rules.validate()?;
        let ids: std::collections::HashSet<_> = players.iter().map(|p| &p.id).collect();
        if players.len() != rules.seats || ids.len() != players.len() {
            return Err(GameError::InvalidRules);
        }
        let mut deck: Vec<u8> = (0..52).collect();
        deck.shuffle(&mut ChaCha20Rng::from_seed(seed));
        let bids = vec![None; rules.seats];
        let mut game = Self {
            rules,
            players,
            round: 0,
            pot: 0,
            seed,
            deck,
            bids,
            finished: false,
            events: Vec::new(),
        };
        game.emit("started", serde_json::json!({"commitment":hex::encode(Sha256::digest(seed)), "rules_version":RULES_VERSION}));
        Ok(game)
    }
    fn emit(&mut self, kind: &str, data: serde_json::Value) {
        self.events.push(Event {
            seq: self.events.len() as u64 + 1,
            round: self.round,
            kind: kind.into(),
            data,
        });
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            rules_version: RULES_VERSION.into(),
            rules: self.rules.clone(),
            round: self.round,
            card: if self.finished {
                None
            } else {
                Some(self.deck[self.round])
            },
            players: self.players.clone(),
            pot: self.pot,
            finished: self.finished,
            commitment: hex::encode(Sha256::digest(self.seed)),
            seed: if self.finished {
                Some(hex::encode(self.seed))
            } else {
                None
            },
            seq: self.events.len() as u64,
        }
    }
    fn check(&self, seat: usize, round: usize) -> Result<(), GameError> {
        if self.finished {
            Err(GameError::Finished)
        } else if round != self.round {
            Err(GameError::StaleRound)
        } else if seat >= self.players.len() {
            Err(GameError::NotSeated)
        } else {
            Ok(())
        }
    }
    pub fn bid(&mut self, seat: usize, round: usize, amount: u32) -> Result<(), GameError> {
        self.check(seat, round)?;
        if self.bids[seat].is_some() {
            return Err(GameError::AlreadyBid);
        }
        if amount > self.players[seat].balance {
            return Err(GameError::InsufficientBalance);
        }
        if amount > 0 && self.players[seat].cards.len() >= 10 {
            return Err(GameError::InventoryFull);
        }
        self.bids[seat] = Some(amount);
        Ok(())
    }
    pub fn cash(&mut self, seat: usize, round: usize, cards: [u8; 5]) -> Result<(), GameError> {
        self.check(seat, round)?;
        if !valid_cards(&cards) || cards.iter().any(|c| !self.players[seat].cards.contains(c)) {
            return Err(GameError::InvalidCards);
        }
        self.consume(seat, cards);
        let hand = evaluate(cards);
        self.reward(seat, hand.reward);
        self.emit(
            "cash_in",
            serde_json::json!({"seat":seat,"hand":hand,"name":hand.name()}),
        );
        Ok(())
    }
    fn consume(&mut self, seat: usize, cards: [u8; 5]) {
        self.players[seat].cards.retain(|c| !cards.contains(c));
    }
    fn reward(&mut self, seat: usize, amount: u32) {
        self.players[seat].balance += amount;
        self.players[seat].score += amount;
    }
    fn tie_key(&self, seat: usize) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"pokerdraft:tie:v1");
        h.update(self.seed);
        h.update((self.round as u64).to_le_bytes());
        h.update((seat as u64).to_le_bytes());
        h.finalize().into()
    }
    pub fn settle(&mut self) -> Result<(), GameError> {
        if self.finished {
            return Err(GameError::Finished);
        }
        let mut bids: Vec<_> = self
            .bids
            .iter()
            .enumerate()
            .filter_map(|(seat, b)| {
                b.filter(|&v| v > 0 && self.players[seat].cards.len() < 10)
                    .map(|v| (seat, v))
            })
            .collect();
        bids.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| self.tie_key(a.0).cmp(&self.tie_key(b.0)))
        });
        let card = self.deck[self.round];
        if let Some(&(seat, amount)) = bids.first() {
            let price = match self.rules.pricing {
                Pricing::FirstPrice => amount,
                Pricing::SecondPrice => bids.get(1).map_or(1, |b| b.1).max(1),
            };
            self.players[seat].balance -= price;
            self.players[seat].cards.push(card);
            self.pot += price;
            self.emit(
                "award",
                serde_json::json!({"card":card,"seat":seat,"price":price}),
            );
        } else {
            self.emit("burn", serde_json::json!({"card":card}));
        }
        self.round += 1;
        self.bids.fill(None);
        if self.round.is_multiple_of(13) {
            self.showdown();
        }
        if self.round == 52 {
            for seat in 0..self.players.len() {
                while let Some(hand) = best(&self.players[seat].cards) {
                    self.consume(seat, hand.cards);
                    self.reward(seat, hand.reward);
                    self.emit("liquidation", serde_json::json!({"seat":seat,"hand":hand}));
                }
            }
            self.finished = true;
            self.emit("finished",serde_json::json!({"seed":hex::encode(self.seed),"scores":self.players.iter().map(|p|p.score).collect::<Vec<_>>(),"retired_pot":self.pot}));
        }
        Ok(())
    }
    fn showdown(&mut self) {
        self.pot += 40;
        let hands: Vec<_> = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(seat, p)| best(&p.cards).map(|h| (seat, h)))
            .collect();
        for &(seat, hand) in &hands {
            self.consume(seat, hand.cards);
        }
        let highest = hands.iter().map(|(_, h)| h.rank).max();
        let mut winners: Vec<_> = hands
            .iter()
            .filter(|(_, h)| Some(h.rank) == highest)
            .map(|(seat, _)| *seat)
            .collect();
        winners.sort_by_key(|&seat| self.tie_key(seat));
        let pot = self.pot;
        let mut payouts = Vec::new();
        for (i, &seat) in winners.iter().enumerate() {
            let amount =
                pot / winners.len() as u32 + u32::from(i < (pot % winners.len() as u32) as usize);
            self.reward(seat, amount);
            payouts.push((seat, amount));
        }
        if !winners.is_empty() {
            self.pot = 0;
        }
        self.emit(
            "showdown",
            serde_json::json!({"hands":hands,"payouts":payouts,"pot":pot}),
        );
    }
    pub fn abort(&mut self, reason: &str) {
        self.finished = true;
        self.emit(
            "aborted",
            serde_json::json!({"reason":reason,"seed":hex::encode(self.seed)}),
        );
    }
}

/// A small transparent baseline, not a claimed optimal strategy.
pub fn reference_bid(game: &Snapshot, seat: usize) -> u32 {
    let p = &game.players[seat];
    if p.cards.len() >= 10 || p.balance == 0 || game.finished {
        return 0;
    }
    let card = game.card.unwrap();
    let pairs = p.cards.iter().filter(|&&c| c % 13 == card % 13).count() as u32;
    let suited = p.cards.iter().filter(|&&c| c / 13 == card / 13).count() as u32;
    let neighbors = p
        .cards
        .iter()
        .filter(|&&c| (c % 13).abs_diff(card % 13) <= 2)
        .count() as u32;
    let mut value = 2 + pairs * 5 + suited + neighbors;
    let mut next = p.cards.clone();
    next.push(card);
    if let Some(hand) = best(&next) {
        value += hand.reward / 5;
    }
    // Stable diversification; no privileged knowledge of the deck or live bids.
    value += ((game.round + seat * 7) % 4) as u32;
    value.min(p.balance / 3 + 1).min(p.balance)
}
pub fn play_house(game: &mut Game, seat: usize) {
    if let Some(hand) = best(&game.players[seat].cards)
        && (hand.reward >= 35 || game.players[seat].cards.len() >= 9)
    {
        let _ = game.cash(seat, game.round, hand.cards);
    }
    let amount = reference_bid(&game.snapshot(), seat);
    let _ = game.bid(seat, game.round, amount);
}

pub fn elo_updates(ratings: &[f64], scores: &[u32]) -> Vec<f64> {
    assert_eq!(ratings.len(), scores.len());
    let n = ratings.len();
    if n < 2 {
        return ratings.to_vec();
    }
    (0..n)
        .map(|i| {
            let mut delta = 0.;
            for j in 0..n {
                if i != j {
                    let expected = 1. / (1. + 10f64.powf((ratings[j] - ratings[i]) / 400.));
                    let actual = match scores[i].cmp(&scores[j]) {
                        std::cmp::Ordering::Greater => 1.,
                        std::cmp::Ordering::Equal => 0.5,
                        _ => 0.,
                    };
                    delta += actual - expected;
                }
            }
            ratings[i] + 32. * delta / (n - 1) as f64
        })
        .collect()
}
