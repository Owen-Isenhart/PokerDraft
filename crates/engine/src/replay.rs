//! Audit public economic transitions and committed shuffle, not private bids.
use crate::{
    Game, Player, Rules, Snapshot,
    poker::{best, evaluate, valid_cards},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn require(ok: bool, reason: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(reason.into()) }
}
fn consume(p: &mut Player, cards: [u8; 5]) -> Result<(), String> {
    require(
        valid_cards(&cards) && cards.iter().all(|c| p.cards.contains(c)),
        "consumed cards are not uniquely owned",
    )?;
    p.cards.retain(|c| !cards.contains(c));
    Ok(())
}
fn reward(p: &mut Player, amount: u32) {
    p.balance += amount;
    p.score += amount;
}
fn seat(data: &Value, n: usize) -> Result<usize, String> {
    let i = data.as_u64().ok_or("invalid seat")? as usize;
    require(i < n, "seat out of range")?;
    Ok(i)
}
fn hand(data: &Value) -> Result<crate::poker::Hand, String> {
    let h: crate::poker::Hand = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
    require(valid_cards(&h.cards), "invalid hand IDs")?;
    require(h == evaluate(h.cards), "hand rank or reward mismatch")?;
    Ok(h)
}
pub fn verify(v: Value) -> Result<Value, String> {
    let r = if v.get("result").is_some() {
        &v["result"]
    } else {
        &v
    };
    let snapshot: Snapshot =
        serde_json::from_value(r["snapshot"].clone()).map_err(|e| e.to_string())?;
    let seed = snapshot
        .seed
        .as_ref()
        .ok_or("seed is still sealed; wait for completion or abort")?;
    let seed: [u8; 32] = hex::decode(seed)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "invalid seed length")?;
    require(
        hex::encode(Sha256::digest(seed)) == snapshot.commitment,
        "seed commitment mismatch",
    )?;
    require(snapshot.rules_version == "1", "unsupported rules version")?;
    let players: Vec<_> = snapshot
        .players
        .iter()
        .map(|p| Player::new(p.id.clone(), p.name.clone(), p.house))
        .collect();
    let mut shuffle = Game::new(
        Rules {
            ..snapshot.rules.clone()
        },
        players.clone(),
        seed,
    )
    .map_err(|e| e.to_string())?;
    let mut deck = Vec::new();
    for _ in 0..52 {
        deck.push(shuffle.snapshot().card.unwrap());
        shuffle.settle().map_err(|e| e.to_string())?;
    }
    let events = r["events"].as_array().ok_or("missing events")?;
    require(events.len() <= 128, "too many events")?;
    require(
        events.first().is_some_and(|e| e["kind"] == "started"),
        "missing initial commitment event",
    )?;
    let history = r["history"].as_array().ok_or("missing history")?;
    require(
        !history.is_empty() && history.len() <= 128,
        "invalid history length",
    )?;
    require(history[0]["seq"] == 1, "missing initial history")?;
    let mut history_index = 0usize;
    let mut players = players;
    let mut drawn = 0;
    let mut pot = 0u32;
    let mut showdowns = 0;
    let mut finished = false;
    for (ix, e) in events.iter().enumerate() {
        require(
            e["seq"].as_u64() == Some(ix as u64 + 1),
            "event sequence gap",
        )?;
        require(!finished, "event after terminal event")?;
        require(
            e["round"].as_u64() == Some(drawn as u64),
            "event round mismatch",
        )?;
        let showdown_due = drawn > 0 && drawn % 13 == 0 && showdowns < drawn / 13;
        let d = &e["data"];
        let kind = e["kind"].as_str().ok_or("event has no kind")?;
        match kind {
            "started" => require(
                ix == 0 && d["commitment"] == snapshot.commitment && d["rules_version"] == "1",
                "invalid initial commitment",
            )?,
            "award" | "burn" => {
                require(
                    !showdown_due
                        && drawn < 52
                        && e["round"].as_u64() == Some(drawn as u64)
                        && d["card"].as_u64() == Some(deck[drawn] as u64),
                    "shuffle order mismatch",
                )?;
                if kind == "award" {
                    let i = seat(&d["seat"], players.len())?;
                    let price = u32::try_from(d["price"].as_u64().ok_or("invalid price")?)
                        .map_err(|_| "price overflow")?;
                    require(
                        price >= 1 && price <= players[i].balance && players[i].cards.len() < 10,
                        "invalid award accounting",
                    )?;
                    players[i].balance -= price;
                    players[i].cards.push(deck[drawn]);
                    pot += price;
                }
                drawn += 1;
            }
            "cash_in" | "liquidation" => {
                let i = seat(&d["seat"], players.len())?;
                let h = hand(&d["hand"])?;
                require(!showdown_due, "missing showdown before consumption")?;
                require(
                    if kind == "liquidation" {
                        drawn == 52 && showdowns == 4
                    } else {
                        drawn < 52
                    },
                    "invalid consumption phase",
                )?;
                if kind == "liquidation" {
                    require(
                        best(&players[i].cards).is_some_and(|b| b.rank == h.rank),
                        "liquidation must select best hand",
                    )?;
                }
                consume(&mut players[i], h.cards)?;
                reward(&mut players[i], h.reward);
            }
            "showdown" => {
                require(showdown_due, "duplicate or premature showdown")?;
                showdowns += 1;
                require(
                    drawn > 0 && drawn % 13 == 0 && e["round"].as_u64() == Some(drawn as u64),
                    "showdown timing mismatch",
                )?;
                pot += 40;
                require(
                    d["pot"].as_u64() == Some(pot as u64),
                    "showdown pot mismatch",
                )?;
                let hands = d["hands"].as_array().ok_or("missing showdown hands")?;
                let qualifying = players.iter().filter(|p| p.cards.len() >= 5).count();
                require(qualifying == hands.len(), "missing showdown entrant")?;
                let mut ranks = Vec::new();
                let mut seats = std::collections::HashSet::new();
                for entry in hands {
                    let i = seat(&entry[0], players.len())?;
                    let h = hand(&entry[1])?;
                    require(seats.insert(i), "duplicate showdown entrant")?;
                    require(
                        best(&players[i].cards).is_some_and(|b| b.rank == h.rank),
                        "showdown must select best hand",
                    )?;
                    consume(&mut players[i], h.cards)?;
                    ranks.push((i, h.rank));
                }
                let high = ranks.iter().map(|p| p.1).max();
                let mut winners: Vec<_> = ranks
                    .iter()
                    .filter(|p| Some(p.1) == high)
                    .map(|p| p.0)
                    .collect();
                winners.sort_by_key(|i| {
                    let mut h = Sha256::new();
                    h.update(b"pokerdraft:tie:v1");
                    h.update(seed);
                    h.update((drawn as u64).to_le_bytes());
                    h.update((*i as u64).to_le_bytes());
                    <[u8; 32]>::from(h.finalize())
                });
                let payouts = d["payouts"].as_array().ok_or("missing payouts")?;
                require(payouts.len() == winners.len(), "payout count mismatch")?;
                for (i, &winner) in winners.iter().enumerate() {
                    let expected = pot / winners.len() as u32
                        + u32::from(i < (pot % winners.len() as u32) as usize);
                    require(
                        payouts[i] == json!([winner, expected]),
                        "split pot mismatch",
                    )?;
                    reward(&mut players[winner], expected);
                }
                if !winners.is_empty() {
                    pot = 0;
                }
            }
            "finished" => {
                require(
                    drawn == 52 && showdowns == 4 && players.iter().all(|p| p.cards.len() < 5),
                    "incomplete match marked finished",
                )?;
                require(
                    d["seed"] == hex::encode(seed)
                        && d["retired_pot"] == pot
                        && d["scores"]
                            == json!(players.iter().map(|p| p.score).collect::<Vec<_>>()),
                    "invalid finish accounting",
                )?;
                require(r["status"] == "complete", "terminal status mismatch")?;
                finished = true;
            }
            "aborted" => {
                require(
                    drawn < 52
                        && !showdown_due
                        && d["seed"] == hex::encode(seed)
                        && r["status"] == "aborted",
                    "invalid abort",
                )?;
                finished = true;
            }
            _ => return Err(format!("unknown event kind: {kind}")),
        }
        if history_index < history.len()
            && history[history_index]["seq"].as_u64() == Some(ix as u64 + 1)
        {
            let mut expected = snapshot.clone();
            expected.round = drawn;
            expected.players = players.clone();
            expected.pot = pot;
            expected.finished = finished;
            expected.card = if finished {
                None
            } else {
                deck.get(drawn).copied()
            };
            expected.seed = if finished {
                Some(hex::encode(seed))
            } else {
                None
            };
            expected.seq = ix as u64 + 1;
            require(
                history[history_index]
                    == serde_json::to_value(expected).map_err(|e| e.to_string())?,
                "history snapshot mismatch",
            )?;
            history_index += 1;
        }
    }
    require(
        history_index == history.len()
            && history.last().is_some_and(|h| h["seq"] == events.len())
            && snapshot.seq == events.len() as u64
            && snapshot.round == drawn,
        "incomplete or nonmonotonic history",
    )?;
    require(finished && snapshot.finished, "replay is not terminal")?;
    require(pot == snapshot.pot, "final pot mismatch")?;
    for (actual, expected) in players.iter().zip(&snapshot.players) {
        require(
            actual.balance == expected.balance
                && actual.score == expected.score
                && actual.cards == expected.cards,
            "final inventory/balance/score mismatch",
        )?;
    }
    Ok(
        json!({"verified":true,"cards":drawn,"showdowns":showdowns,"shuffle":true,"public_economy":true,"private_bid_allocation_verified":false}),
    )
}
