//! Persistent, deterministic heads-up Swiss → top-cut → best-of-three finals.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entrant {
    pub bot: String,
    pub points: u32,
    pub opponents: Vec<String>,
    pub byes: u32,
    pub seed: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pairing {
    pub id: usize,
    pub a: String,
    pub b: Option<String>,
    pub winner: Option<String>,
    pub match_id: Option<String>,
    pub wins: [u8; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tournament {
    pub id: String,
    pub name: String,
    pub state: String,
    pub swiss_rounds: usize,
    pub cut: usize,
    pub round: usize,
    pub entrants: Vec<Entrant>,
    pub pairings: Vec<Pairing>,
    pub history: Vec<Pairing>,
    pub champion: Option<String>,
}
impl Tournament {
    pub fn new(id: String, name: String, swiss_rounds: usize, cut: usize) -> Result<Self, String> {
        if !(1..=12).contains(&swiss_rounds) || !(2..=32).contains(&cut) || !cut.is_power_of_two() {
            return Err("invalid_tournament_config".into());
        }
        Ok(Self {
            id,
            name,
            state: "registration".into(),
            swiss_rounds,
            cut,
            round: 0,
            entrants: vec![],
            pairings: vec![],
            history: vec![],
            champion: None,
        })
    }
    pub fn register(&mut self, bot: String) -> Result<(), String> {
        if self.state != "registration" {
            return Err("registration_closed".into());
        }
        if self.entrants.len() >= 64 || self.entrants.iter().any(|e| e.bot == bot) {
            return Err("duplicate_or_full".into());
        }
        self.entrants.push(Entrant {
            bot,
            points: 0,
            opponents: vec![],
            byes: 0,
            seed: self
                .entrants
                .iter()
                .map(|e| e.seed)
                .max()
                .map_or(0, |s| s + 1),
        });
        Ok(())
    }
    pub fn buchholz(&self, e: &Entrant) -> u32 {
        e.opponents
            .iter()
            .filter_map(|id| self.entrants.iter().find(|p| &p.bot == id))
            .map(|p| p.points)
            .sum()
    }
    pub fn standings(&self) -> Vec<&Entrant> {
        let mut v: Vec<_> = self.entrants.iter().collect();
        v.sort_by(|a, b| {
            b.points
                .cmp(&a.points)
                .then_with(|| self.buchholz(b).cmp(&self.buchholz(a)))
                .then(a.seed.cmp(&b.seed))
        });
        v
    }
    pub fn start(&mut self) -> Result<(), String> {
        if self.state != "registration" || self.entrants.len() < self.cut {
            return Err("not_ready".into());
        }
        self.state = "swiss".into();
        self.swiss();
        Ok(())
    }
    fn make(&mut self, a: String, b: Option<String>) {
        self.pairings.push(Pairing {
            id: self.history.len() + self.pairings.len(),
            a: a.clone(),
            b: b.clone(),
            winner: if b.is_none() { Some(a) } else { None },
            match_id: None,
            wins: [0, 0],
        });
    }
    fn swiss(&mut self) {
        self.round += 1;
        self.pairings.clear();
        let mut ids: Vec<_> = self.standings().iter().map(|e| e.bot.clone()).collect();
        if ids.len() % 2 == 1 {
            let ix = (0..ids.len())
                .rev()
                .min_by_key(|&i| self.entrants.iter().find(|e| e.bot == ids[i]).unwrap().byes)
                .unwrap();
            let bye = ids.remove(ix);
            let e = self.entrants.iter_mut().find(|e| e.bot == bye).unwrap();
            e.byes += 1;
            e.points += 3;
            self.make(bye, None);
        }
        while !ids.is_empty() {
            let a = ids.remove(0);
            let old = &self.entrants.iter().find(|e| e.bot == a).unwrap().opponents;
            let ix = ids.iter().position(|id| !old.contains(id)).unwrap_or(0);
            let b = ids.remove(ix);
            self.make(a, Some(b));
        }
    }
    pub fn result(&mut self, index: usize, winner: Option<String>) -> Result<(), String> {
        if !["swiss", "elimination", "final"].contains(&self.state.as_str()) {
            return Err("not_running".into());
        }
        let p = self.pairings.get_mut(index).ok_or("unknown_pairing")?;
        if p.winner.is_some() {
            return Err("already_resolved".into());
        }
        p.match_id = None;
        let Some(winner) = winner else {
            return Ok(());
        }; // Draw: replay, no arbitrary seat winner.
        if winner != p.a && Some(&winner) != p.b.as_ref() {
            return Err("invalid_winner".into());
        }
        let side = usize::from(winner != p.a);
        p.wins[side] += 1;
        if self.state == "final" && p.wins[side] < 2 {
            return Ok(());
        }
        p.winner = Some(winner.clone());
        if self.state == "swiss" {
            let a = p.a.clone();
            let b = p.b.clone().unwrap();
            for e in &mut self.entrants {
                if e.bot == a {
                    e.opponents.push(b.clone());
                } else if e.bot == b {
                    e.opponents.push(a.clone());
                }
                if e.bot == winner {
                    e.points += 3;
                }
            }
        }
        if self.pairings.iter().all(|p| p.winner.is_some()) {
            self.advance();
        }
        Ok(())
    }
    fn advance(&mut self) {
        let mut winners: Vec<String> = self
            .pairings
            .iter()
            .filter_map(|p| p.winner.clone())
            .collect();
        self.history.extend(self.pairings.clone());
        self.pairings.clear();
        if self.state == "swiss" && self.round < self.swiss_rounds {
            self.swiss();
            return;
        }
        if self.state == "swiss" {
            winners = self
                .standings()
                .iter()
                .take(self.cut)
                .map(|e| e.bot.clone())
                .collect();
            // Standard high-v-low seeding at cut.
            let mut seeded = Vec::new();
            while !winners.is_empty() {
                seeded.push(winners.remove(0));
                seeded.push(winners.pop().unwrap());
            }
            winners = seeded;
        } else if winners.len() == 1 {
            self.champion = winners.pop();
            self.state = "complete".into();
            return;
        }
        self.round += 1;
        self.state = if winners.len() == 2 {
            "final"
        } else {
            "elimination"
        }
        .into();
        for p in winners.chunks_exact(2) {
            self.make(p[0].clone(), Some(p[1].clone()));
        }
    }
    pub fn occupied(&self) -> HashSet<String> {
        if ["complete", "cancelled"].contains(&self.state.as_str()) {
            HashSet::new()
        } else {
            self.entrants.iter().map(|e| e.bot.clone()).collect()
        }
    }
}
