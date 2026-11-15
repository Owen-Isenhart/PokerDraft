//! Reproducible storage/auth/fan-out profiling, independent of network latency.
// Included by both targets with the profiling feature; only the profile binary invokes it.
#![allow(dead_code)]
use super::*;
fn timed(mut f: impl FnMut(), n: usize) -> Value {
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        f();
        samples.push(start.elapsed().as_nanos() as u64);
    }
    samples.sort_unstable();
    json!({"samples":n,"p50_ns":samples[n/2],"p95_ns":samples[n*95/100]})
}
pub fn run() {
    let count = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1000)
        .clamp(100, 10000);
    let dir = std::env::temp_dir().join(format!("pokerdraft-profile-{}", id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("profile.db");
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let (feed, _) = broadcast::channel(512);
    let mut world = World::default();
    for i in 0..1000 {
        world.teams.push(Team {
            id: format!("team-{i:04}"),
            name: format!("Fixture Team {i}"),
            owner_hash: hash(&format!("fixture-owner-{i}")),
            disabled: false,
            created_ms: 0,
        });
        world.bots.push(Bot {
            id: format!("bot-{i:04}"),
            team_id: format!("team-{i:04}"),
            name: "v1".into(),
            token_hash: hash(&format!("fixture-token-{i}")),
            rating: 1500.,
            games: 10,
            disabled: false,
            created_ms: 0,
        });
    }
    let mut s = Service {
        store,
        indexes: Indexes::new(&world),
        world,
        live: HashMap::new(),
        queue: VecDeque::new(),
        lobby_deadline: Instant::now() + Duration::from_secs(30),
        lobby_deadline_ms: now_ms() + 30000,
        feed,
        admin_hash: None,
        rules: Rules {
            seats: 2,
            auction_ms: 250,
            ..Rules::default()
        },
        max_live: 16,
        demo: false,
        retries: HashMap::new(),
        retry_bytes: 0,
        retry_order: VecDeque::new(),
        commands: 0,
        rejected: 0,
        max_tick_lag_ms: 0,
    };
    s.persist(&[]).unwrap();
    for i in 0..count {
        let players = [i % 999, (i + 1) % 999]
            .iter()
            .map(|i| Player::new(format!("bot-{i:04}"), format!("Fixture {i}"), false))
            .collect();
        let mut game = Game::new(s.rules.clone(), players, [77; 32]).unwrap();
        let mut history = vec![game.snapshot()];
        for _ in 0..52 {
            game.settle().unwrap();
            history.push(game.snapshot());
        }
        let record = json!({"id":format!("fixture-{i:06}"),"status":"complete","created_ms":i as u64,"updated_ms":i as u64,"snapshot":game.snapshot(),"events":game.events,"history":history,"private_seed":hex::encode([77;32]),"tournament":null});
        s.store.save(None, &[record]).unwrap();
    }
    let auth = timed(
        || {
            std::hint::black_box(s.bot("fixture-token-999").unwrap());
        },
        10000,
    );
    let list = timed(
        || {
            std::hint::black_box(s.read("leaderboard", "", 0).unwrap());
        },
        50,
    );
    let cold_start = Instant::now();
    let first_page = s.store.archive("leaderboard", "", 0).unwrap();
    assert_eq!(first_page["bots"].as_array().unwrap().len(), 100);
    let metadata_cold_ns = cold_start.elapsed().as_nanos() as u64;
    let metadata_cached = timed(
        || {
            std::hint::black_box(s.store.archive("leaderboard", "", 0).unwrap());
        },
        100,
    );
    let history_miss = timed(
        || {
            let records = s.store.history_for(&["bot-0999".into()], 0).unwrap();
            assert!(records.is_empty());
            std::hint::black_box(records);
        },
        100,
    );
    let history_hit = timed(
        || {
            std::hint::black_box(s.store.history_for(&["bot-0000".into()], 0).unwrap());
        },
        100,
    );
    s.start_match_seeded(
        vec!["bot-0000".into(), "bot-0001".into()],
        None,
        false,
        [88; 32],
    )
    .unwrap();
    let mid = s.live.keys().next().unwrap().clone();
    let mut write_times = Vec::new();
    let mut encoded_bytes = 0;
    for _ in 0..52 {
        let start = Instant::now();
        let l = s.live.get_mut(&mid).unwrap();
        for i in 0..2 {
            play_house(&mut l.game, i);
        }
        l.game.settle().unwrap();
        l.history.push(l.game.snapshot());
        let record = l.record(
            &mid,
            if l.game.snapshot().finished {
                "complete"
            } else {
                "live"
            },
        );
        s.store.save(None, std::slice::from_ref(&record)).unwrap();
        s.live.get_mut(&mid).unwrap().mark_persisted();
        write_times.push(start.elapsed().as_nanos() as u64);
        encoded_bytes += record["history"].to_string().len() + record["events"].to_string().len();
    }
    write_times.sort_unstable();
    let frame = Feed::new(
        &format!("match:{mid}"),
        vec!["bot-0000".into(), "bot-0001".into()],
        json!({"type":"match","match":s.live[&mid].public(&mid)}),
    );
    let fanout = timed(
        || {
            let f = frame.clone();
            std::hint::black_box(f.message());
        },
        10000,
    );
    println!(
        "{}",
        json!({"fixture_matches":count,"fixture_accounts":1000,"sqlite":rusqlite::version(),"auth_lookup":auth,"leaderboard":list,"public_metadata_first_page_ns":metadata_cold_ns,"public_metadata_cached_page":metadata_cached,"history_miss":history_miss,"history_hit":history_hit,"round_write":{"p50_ns":write_times[26],"p95_ns":write_times[49],"encoded_history_event_bytes":encoded_bytes},"fanout_per_receiver":fanout})
    );
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}
