use pokerdraft_engine::{poker::*, *};
fn game(seats: usize) -> Game {
    Game::new(
        Rules {
            seats,
            ..Rules::default()
        },
        (0..seats)
            .map(|i| Player::new(i.to_string(), format!("Bot {i}"), false))
            .collect(),
        [42; 32],
    )
    .unwrap()
}
#[test]
fn auction_sealed_and_second_price() {
    let mut g = game(2);
    let before = serde_json::to_string(&g.snapshot()).unwrap();
    g.bid(0, 0, 90).unwrap();
    assert_eq!(serde_json::to_string(&g.snapshot()).unwrap(), before);
    assert_eq!(g.bid(0, 0, 1), Err(GameError::AlreadyBid));
    g.bid(1, 0, 20).unwrap();
    g.settle().unwrap();
    assert_eq!(g.players[0].balance, 80);
    assert_eq!(g.pot, 20);
}
#[test]
fn bad_inputs_are_atomic() {
    let mut g = game(2);
    assert_eq!(g.bid(0, 0, 101), Err(GameError::InsufficientBalance));
    assert_eq!(g.bid(2, 0, 1), Err(GameError::NotSeated));
    g.players[0].cards = vec![0, 1, 2, 3, 4];
    let before = serde_json::to_string(&g.snapshot()).unwrap();
    assert!(g.cash(0, 0, [0, 0, 1, 2, 3]).is_err());
    assert!(g.cash(0, 0, [0, 1, 2, 3, 52]).is_err());
    assert!(g.cash(0, 1, [0, 1, 2, 3, 4]).is_err());
    assert_eq!(serde_json::to_string(&g.snapshot()).unwrap(), before);
    g.cash(0, 0, [0, 1, 2, 3, 4]).unwrap();
    assert_eq!(g.players[0].score, 400);
    assert!(g.players[0].cards.is_empty());
}
#[test]
fn ties_independent_of_arrival_and_first_price() {
    let mut a = game(2);
    let mut b = a.clone();
    a.bid(0, 0, 11).unwrap();
    a.bid(1, 0, 11).unwrap();
    b.bid(1, 0, 11).unwrap();
    b.bid(0, 0, 11).unwrap();
    a.settle().unwrap();
    b.settle().unwrap();
    assert_eq!(
        serde_json::to_value(a.snapshot()).unwrap(),
        serde_json::to_value(b.snapshot()).unwrap()
    );
    let mut g = game(2);
    g.rules.pricing = Pricing::FirstPrice;
    g.bid(0, 0, 15).unwrap();
    g.settle().unwrap();
    assert_eq!(g.players[0].balance, 85);
}
#[test]
fn all_pass_finishes_and_reveals_seed() {
    let mut g = game(2);
    assert!(g.snapshot().seed.is_none());
    for _ in 0..52 {
        g.settle().unwrap();
    }
    let s = g.snapshot();
    assert!(s.finished);
    assert_eq!(s.seed, Some(hex::encode([42; 32])));
    assert_eq!(s.pot, 160);
    assert!(g.settle().is_err());
}
#[test]
fn full_inventory_rejected() {
    let mut g = game(2);
    g.players[0].cards = (0..10).collect();
    assert_eq!(g.bid(0, 0, 1), Err(GameError::InventoryFull));
    g.bid(0, 0, 0).unwrap();
}
#[test]
fn kicker_and_wheel() {
    let wheel = evaluate([12, 0, 1, 2, 3]);
    let six = evaluate([0, 1, 2, 3, 4]);
    assert_eq!(wheel.category, 8);
    assert!(six.rank > wheel.rank);
    assert_eq!(evaluate([8, 9, 10, 11, 12]).reward, 600);
    assert!(evaluate([12, 25, 11, 10, 9]).rank > evaluate([11, 24, 12, 10, 9]).rank);
}
#[test]
fn seeded_simulation_invariants() {
    for seed in 0..300u64 {
        let mut g = Game::new(
            Rules::default(),
            (0..6)
                .map(|i| Player::new(i.to_string(), i.to_string(), true))
                .collect(),
            sha2_seed(seed),
        )
        .unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..52 {
            let c = g.snapshot().card.unwrap();
            assert!(seen.insert(c));
            for seat in 0..6 {
                play_house(&mut g, seat);
            }
            g.settle().unwrap();
            let all: Vec<_> = g.players.iter().flat_map(|p| p.cards.clone()).collect();
            assert!(valid_cards(&all));
            assert!(g.players.iter().all(|p| p.cards.len() <= 10));
        }
        assert_eq!(seen.len(), 52);
        assert!(g.snapshot().finished);
    }
}
fn sha2_seed(v: u64) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(v.to_le_bytes()).into()
}
#[test]
fn elo_zero_sum_ties() {
    let r = elo_updates(&[1500., 1500., 1500.], &[10, 10, 0]);
    assert_eq!(r[0], r[1]);
    assert!((r.iter().sum::<f64>() - 4500.).abs() < 1e-8);
}
#[test]
fn exhaustive_five_card_categories() {
    let mut counts = [0u32; 9];
    let mut unique_ranks = std::collections::HashSet::new();
    for a in 0..48 {
        for b in a + 1..49 {
            for c in b + 1..50 {
                for d in c + 1..51 {
                    for e in d + 1..52 {
                        let hand = evaluate([a, b, c, d, e]);
                        counts[hand.category as usize] += 1;
                        unique_ranks.insert(hand.rank);
                    }
                }
            }
        }
    }
    assert_eq!(unique_ranks.len(), 7462);
    assert_eq!(
        counts,
        [1302540, 1098240, 123552, 54912, 10200, 5108, 3744, 624, 40]
    );
}
#[test]
fn tournament_full_lifecycle() {
    let mut t = tournament::Tournament::new("t".into(), "Cup".into(), 3, 4).unwrap();
    for i in 0..7 {
        t.register(i.to_string()).unwrap();
    }
    t.start().unwrap();
    let mut steps = 0;
    while t.state != "complete" {
        let ix = t.pairings.iter().position(|p| p.winner.is_none()).unwrap();
        let a = t.pairings[ix].a.clone();
        t.result(ix, Some(a)).unwrap();
        steps += 1;
        assert!(steps < 30);
    }
    assert!(t.champion.is_some());
    assert!(t.entrants.iter().all(|e| e.byes <= 1));
}
#[test]
fn tournament_draw_replay_and_final_series() {
    let mut t = tournament::Tournament::new("t".into(), "Cup".into(), 1, 2).unwrap();
    t.register("a".into()).unwrap();
    t.register("b".into()).unwrap();
    t.start().unwrap();
    t.result(0, None).unwrap();
    assert_eq!(t.state, "swiss");
    t.result(0, Some("a".into())).unwrap();
    assert_eq!(t.state, "final");
    t.result(0, Some("b".into())).unwrap();
    assert_eq!(t.state, "final");
    t.result(0, Some("a".into())).unwrap();
    t.result(0, Some("b".into())).unwrap();
    assert_eq!(t.champion, Some("b".into()));
}

#[test]
fn repeated_draws_suspend_without_inventing_a_winner() {
    let mut t = tournament::Tournament::new("t".into(), "Cup".into(), 1, 2).unwrap();
    t.register("a".into()).unwrap();
    t.register("b".into()).unwrap();
    t.start().unwrap();
    for _ in 0..3 {
        t.result(0, None).unwrap();
    }
    assert_eq!(t.state, "attention");
    assert!(t.champion.is_none());
    assert!(t.pairings[0].winner.is_none());
}
#[test]
fn replay_verifier_enforces_chronology_history_and_terminal_state() {
    let mut g = game(2);
    let mut history = vec![g.snapshot()];
    for _ in 0..52 {
        for i in 0..2 {
            play_house(&mut g, i);
        }
        g.settle().unwrap();
        history.push(g.snapshot());
    }
    let r = serde_json::json!({"status":"complete","snapshot":g.snapshot(),"events":g.events,"history":history});
    assert!(replay::verify(r.clone()).is_ok());
    let mut missing = r.clone();
    missing["events"].as_array_mut().unwrap().remove(0);
    assert!(replay::verify(missing).is_err());
    let mut history = r.clone();
    history["history"][0]["pot"] = serde_json::json!(100);
    assert!(replay::verify(history).is_err());
    let mut early = r.clone();
    let cash = early["events"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["kind"] == "cash_in")
        .unwrap();
    cash["kind"] = serde_json::json!("liquidation");
    assert!(replay::verify(early).is_err());
    let mut terminal = r;
    terminal["status"] = serde_json::json!("aborted");
    assert!(replay::verify(terminal).is_err());
}
