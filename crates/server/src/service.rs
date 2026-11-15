use crate::store::Store;
use pokerdraft_engine::{
    Game, Player, Rules, Snapshot, elo_updates, play_house, tournament::Tournament,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn secret() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}
fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
fn label(s: &str) -> Result<String, String> {
    let s = s.trim();
    if !(2..=40).contains(&s.chars().count())
        || s.chars().any(|c| {
            c.is_control() || matches!(c,'\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        Err("name_must_be_2_to_40_characters".into())
    } else {
        Ok(s.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub owner_hash: String,
    pub disabled: bool,
    pub created_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bot {
    pub id: String,
    pub team_id: String,
    pub name: String,
    pub token_hash: String,
    pub rating: f64,
    pub games: u32,
    pub disabled: bool,
    pub created_ms: u64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct World {
    pub teams: Vec<Team>,
    pub bots: Vec<Bot>,
    pub tournaments: Vec<Tournament>,
    #[serde(default)]
    pub paused: bool,
}
#[derive(Clone)]
pub struct Feed {
    pub topic: std::sync::Arc<str>,
    pub bots: std::sync::Arc<[String]>,
    pub value: axum::extract::ws::Utf8Bytes,
}
impl Feed {
    pub fn new(topic: &str, bots: Vec<String>, value: Value) -> Self {
        Self {
            topic: topic.into(),
            bots: bots.into(),
            value: value.to_string().into(),
        }
    }
    pub fn message(&self) -> axum::extract::ws::Message {
        axum::extract::ws::Message::Text(self.value.clone())
    }
}
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Request>,
    archive: mpsc::Sender<ArchiveRequest>,
    pub feed: broadcast::Sender<Feed>,
}
struct ArchiveRequest {
    kind: String,
    id: String,
    offset: usize,
    reply: oneshot::Sender<Result<Value, String>>,
}
#[derive(Default)]
struct Indexes {
    owners: HashMap<String, usize>,
    tokens: HashMap<String, usize>,
    teams: HashMap<String, usize>,
    bots: HashMap<String, usize>,
    team_bots: HashMap<String, Vec<usize>>,
    team_names: HashSet<String>,
}
impl Indexes {
    fn new(w: &World) -> Self {
        let mut x = Self::default();
        for (i, t) in w.teams.iter().enumerate() {
            x.owners.insert(t.owner_hash.clone(), i);
            x.teams.insert(t.id.clone(), i);
            x.team_names.insert(t.name.to_lowercase());
        }
        for (i, b) in w.bots.iter().enumerate() {
            x.tokens.insert(b.token_hash.clone(), i);
            x.bots.insert(b.id.clone(), i);
            x.team_bots.entry(b.team_id.clone()).or_default().push(i);
        }
        x
    }
}
pub enum Request {
    Read {
        kind: String,
        id: String,
        offset: usize,
        reply: oneshot::Sender<Result<Value, String>>,
    },
    Command {
        token: String,
        body: Value,
        reply: oneshot::Sender<Result<Value, String>>,
    },
    Tick,
    Metrics {
        reply: oneshot::Sender<Result<Value, String>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}
impl Handle {
    pub async fn read(&self, kind: &str, id: &str, offset: usize) -> Result<Value, String> {
        if [
            "replay",
            "matches",
            "bot_matches",
            "team_matches",
            "bots",
            "leaderboard",
            "tournaments",
        ]
        .contains(&kind)
        {
            return self.archive(kind, id, offset).await;
        }
        let (tx, rx) = oneshot::channel();
        self.tx
            .try_send(Request::Read {
                kind: kind.into(),
                id: id.into(),
                offset,
                reply: tx,
            })
            .map_err(|_| "server_busy")?;
        let result = tokio::time::timeout(Duration::from_secs(3), rx)
            .await
            .map_err(|_| "server_busy")?
            .map_err(|_| "service_unavailable")?;
        if result.as_ref().err().is_some_and(|e| e == "archive_lookup") {
            self.archive("match", id, offset).await
        } else {
            result
        }
    }
    async fn archive(&self, kind: &str, id: &str, offset: usize) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel();
        self.archive
            .try_send(ArchiveRequest {
                kind: kind.into(),
                id: id.into(),
                offset,
                reply: tx,
            })
            .map_err(|_| "server_busy")?;
        tokio::time::timeout(Duration::from_secs(3), rx)
            .await
            .map_err(|_| "server_busy")?
            .map_err(|_| "service_unavailable")?
    }
    pub async fn command(&self, token: String, body: Value) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .try_send(Request::Command {
                token,
                body,
                reply: tx,
            })
            .map_err(|_| "server_busy")?;
        tokio::time::timeout(Duration::from_secs(3), rx)
            .await
            .map_err(|_| "server_busy")?
            .map_err(|_| "service_unavailable")?
    }
    pub async fn metrics(&self) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .try_send(Request::Metrics { reply: tx })
            .map_err(|_| "server_busy")?;
        tokio::time::timeout(Duration::from_secs(3), rx)
            .await
            .map_err(|_| "server_busy")?
            .map_err(|_| "service_unavailable")?
    }
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(Request::Shutdown { reply: tx }).await;
        let _ = rx.await;
    }
}
struct Live {
    game: Game,
    seed: String,
    created_ms: u64,
    deadline: Instant,
    deadline_ms: u64,
    history: Vec<Snapshot>,
    tournament: Option<(String, usize)>,
    saved_events: usize,
    saved_history: usize,
}
impl Live {
    fn record(&self, id: &str, status: &str) -> Value {
        json!({"id":id,"status":status,"created_ms":self.created_ms,"updated_ms":now_ms(),"snapshot":self.game.snapshot(),"events":self.game.events[self.saved_events..],"history":self.history[self.saved_history..],"history_start":self.saved_history,"private_seed":self.seed,"tournament":self.tournament.as_ref().map(|t|&t.0)})
    }
    fn mark_persisted(&mut self) {
        self.saved_events = self.game.events.len();
        self.saved_history = self.history.len();
    }
    fn public(&self, id: &str) -> Value {
        json!({"id":id,"status":"live","deadline_ms":self.deadline_ms,"server_time_ms":now_ms(),"snapshot":self.game.snapshot(),"events":self.game.events.iter().rev().take(12).cloned().collect::<Vec<_>>()})
    }
}
pub struct Service {
    store: Store,
    world: World,
    indexes: Indexes,
    live: HashMap<String, Live>,
    queue: VecDeque<String>,
    lobby_deadline: Instant,
    lobby_deadline_ms: u64,
    feed: broadcast::Sender<Feed>,
    admin_hash: Option<String>,
    rules: Rules,
    max_live: usize,
    demo: bool,
    retries: HashMap<String, (String, Value, Instant, usize)>,
    retry_bytes: usize,
    retry_order: VecDeque<String>,
    pub commands: u64,
    pub rejected: u64,
    pub max_tick_lag_ms: u64,
}
impl Service {
    pub fn spawn(
        path: &str,
        admin: Option<String>,
        rules: Rules,
        demo: bool,
    ) -> Result<Handle, String> {
        rules.validate().map_err(|e| e.to_string())?;
        if path == ":memory:" {
            return Err("service_requires_file_database".into());
        }
        let mut store = Store::open(path).map_err(|e| e.to_string())?;
        let mut world = store.world()?;
        let recovered = store.recover(&mut world)?;
        tracing::info!(recovered, "opened durable store");
        let mut reader = Store::reader(path).map_err(|e| e.to_string())?;
        let (archive, mut archive_rx) = mpsc::channel::<ArchiveRequest>(64);
        std::thread::Builder::new()
            .name("public-archive-reader".into())
            .spawn(move || {
                while let Some(req) = archive_rx.blocking_recv() {
                    if !req.reply.is_closed() {
                        let _ = req
                            .reply
                            .send(reader.archive(&req.kind, &req.id, req.offset));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let (tx, mut rx) = mpsc::channel(2048);
        let (feed, _) = broadcast::channel(512);
        let handle = Handle {
            tx: tx.clone(),
            archive,
            feed: feed.clone(),
        };
        let mut service = Self {
            store,
            indexes: Indexes::new(&world),
            world,
            live: HashMap::new(),
            queue: VecDeque::new(),
            lobby_deadline: Instant::now() + Duration::from_secs(30),
            lobby_deadline_ms: now_ms() + 30000,
            feed,
            admin_hash: admin.map(|a| hash(&a)),
            rules,
            max_live: 16,
            demo,
            retries: HashMap::new(),
            retry_bytes: 0,
            retry_order: VecDeque::new(),
            commands: 0,
            rejected: 0,
            max_tick_lag_ms: 0,
        };
        std::thread::Builder::new().name("competition-writer".into()).spawn(move||{
   let mut last_tick=Instant::now();
   while let Some(req)=rx.blocking_recv(){
    if matches!(&req,Request::Tick)||last_tick.elapsed()>=Duration::from_millis(10){if let Err(error)=service.tick(){tracing::error!(%error,"competition tick failed");break;}last_tick=Instant::now();}
    let result:Result<(),String>=match req{
     Request::Read{kind,id,offset,reply}=>{if !reply.is_closed(){let _=reply.send(service.read(&kind,&id,offset));}Ok(())},
     Request::Command{token,body,reply}=>{
      if reply.is_closed(){continue;}
      service.commands+=1;
      let r=service.command(&token,&body);if r.is_err(){service.rejected+=1;}let fatal=r.as_ref().err().is_some_and(|e|e.starts_with("storage_failure:"));let _=reply.send(r);
      if fatal{Err("storage_failure".into())}else{Ok(())}
     },
     Request::Tick=>Ok(()),
     Request::Metrics{reply}=>{let _=reply.send(Ok(service.metrics()));Ok(())},
     Request::Shutdown{reply}=>{let r=service.abort_all("server_shutdown");let _=reply.send(());if let Err(e)=r{tracing::error!(%e,"shutdown persistence failed");}break;}
    };
    if let Err(error)=result{tracing::error!(%error,"competition service stopped to preserve durability");break;}
   }
  }).map_err(|e|e.to_string())?;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) =
                    tx.try_send(Request::Tick)
                {
                    break;
                }
            }
        });
        Ok(handle)
    }
    fn persist(&mut self, records: &[Value]) -> Result<(), String> {
        self.store
            .save(Some(&self.world), records)
            .map_err(|e| format!("storage_failure:{e}"))?;
        self.indexes = Indexes::new(&self.world);
        Ok(())
    }
    fn persist_records(&mut self, records: &[Value]) -> Result<(), String> {
        self.store
            .save(None, records)
            .map_err(|e| format!("storage_failure:{e}"))
    }
    fn publish(&self, topic: &str, bots: Vec<String>, value: Value) {
        let _ = self.feed.send(Feed::new(topic, bots, value));
    }
    fn directory(&self) -> Value {
        let mut matches:Vec<_>=self.live.iter().map(|(id,l)|json!({"id":id,"round":l.game.round,"players":l.game.players.iter().map(|p|json!({"id":p.id,"name":p.name,"house":p.house})).collect::<Vec<_>>(),"tournament":l.tournament.as_ref().map(|t|&t.0)})).collect();
        matches.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        json!({"type":"directory","protocol":1,"server_time_ms":now_ms(),"rules":self.rules,"paused":self.world.paused,"lobby":{"bots":self.queue.iter().filter_map(|id|self.world.bots.iter().find(|b|&b.id==id)).map(|b|self.bot_public(b)).collect::<Vec<_>>(),"deadline_ms":self.lobby_deadline_ms},"matches":matches,"teams":self.world.teams.len(),"registered_bots":self.world.bots.len()})
    }
    fn broadcast_directory(&self) {
        self.publish("directory", vec![], self.directory());
    }
    fn bot_public(&self, b: &Bot) -> Value {
        json!({"id":b.id,"team_id":b.team_id,"team":self.indexes.teams.get(&b.team_id).map(|&i|&self.world.teams[i].name),"name":b.name,"rating":b.rating,"games":b.games,"disabled":b.disabled||self.indexes.teams.get(&b.team_id).is_some_and(|&i|self.world.teams[i].disabled),"created_ms":b.created_ms})
    }
    fn read(&self, kind: &str, id: &str, offset: usize) -> Result<Value, String> {
        if offset > crate::security::MAX_OFFSET {
            return Err("invalid_offset".into());
        }
        match kind {
            "directory" => Ok(self.directory()),
            "health" => Ok(
                json!({"status":"ok","protocol":1,"active_matches":self.live.len(),"max_tick_lag_ms":self.max_tick_lag_ms}),
            ),
            "leaderboard" => {
                let mut bots: Vec<_> = self
                    .world
                    .bots
                    .iter()
                    .filter(|b| {
                        !b.disabled
                            && b.games > 0
                            && self
                                .indexes
                                .teams
                                .get(&b.team_id)
                                .is_some_and(|&i| !self.world.teams[i].disabled)
                    })
                    .collect();
                bots.sort_by(|a, b| b.rating.total_cmp(&a.rating));
                Ok(json!({"bots":bots.into_iter().map(|b|self.bot_public(b)).collect::<Vec<_>>() }))
            }
            "bots" => Ok(
                json!({"bots":self.world.bots.iter().map(|b|self.bot_public(b)).collect::<Vec<_>>()}),
            ),
            "matches" => Ok(
                json!({"matches":self.store.recent(50,offset)?,"offset":offset,"next_offset":offset+50}),
            ),
            "match" => {
                if let Some(l) = self.live.get(id) {
                    Ok(l.public(id))
                } else {
                    Err("archive_lookup".into())
                }
            }
            "replay" => self.store.record(id),
            "bot_matches" | "team_matches" => {
                let ids: Vec<String> = if kind == "bot_matches" {
                    if !self.world.bots.iter().any(|b| b.id == id) {
                        return Err("bot_not_found".into());
                    }
                    vec![id.into()]
                } else {
                    if !self.world.teams.iter().any(|t| t.id == id) {
                        return Err("team_not_found".into());
                    }
                    self.world
                        .bots
                        .iter()
                        .filter(|b| b.team_id == id)
                        .map(|b| b.id.clone())
                        .collect()
                };
                Ok(
                    json!({"matches":self.store.history_for(&ids,offset)?,"offset":offset,"next_offset":offset+50}),
                )
            }
            "tournaments" => Ok(
                json!({"tournaments":self.world.tournaments,"standings":self.world.tournaments.iter().map(|t|json!({"id":t.id,"entries":t.standings().iter().map(|e|json!({"bot":e.bot,"points":e.points,"buchholz":t.buchholz(e),"byes":e.byes})).collect::<Vec<_>>()})).collect::<Vec<_>>()}),
            ),
            _ => Err("not_found".into()),
        }
    }
    fn metrics(&self) -> Value {
        json!({"commands":self.commands,"rejected":self.rejected,"active_matches":self.live.len(),"max_tick_lag_ms":self.max_tick_lag_ms})
    }
    fn owner(&self, token: &str) -> Result<String, String> {
        self.indexes
            .owners
            .get(&hash(token))
            .map(|&i| &self.world.teams[i])
            .filter(|t| !t.disabled)
            .map(|t| t.id.clone())
            .ok_or("owner_auth_required".into())
    }
    fn bot(&self, token: &str) -> Result<String, String> {
        self.indexes
            .tokens
            .get(&hash(token))
            .map(|&i| &self.world.bots[i])
            .filter(|b| {
                !b.disabled
                    && self
                        .indexes
                        .teams
                        .get(&b.team_id)
                        .is_some_and(|&i| !self.world.teams[i].disabled)
            })
            .map(|b| b.id.clone())
            .ok_or("bot_auth_required".into())
    }
    fn admin(&self, token: &str) -> Result<(), String> {
        if self.admin_hash.as_ref().is_some_and(|h| h == &hash(token)) {
            Ok(())
        } else {
            Err("admin_auth_required".into())
        }
    }
    fn busy(&self, bot: &str) -> bool {
        self.queue.iter().any(|b| b == bot)
            || self
                .live
                .values()
                .any(|l| l.game.players.iter().any(|p| p.id == bot))
            || self.world.tournaments.iter().any(|t| {
                !["complete", "cancelled"].contains(&t.state.as_str())
                    && t.entrants.iter().any(|e| e.bot == bot)
            })
    }
    fn team_busy(&self, team: &str, except: Option<&str>) -> bool {
        self.indexes.team_bots.get(team).is_some_and(|bots| {
            bots.iter().any(|&i| {
                let b = &self.world.bots[i];
                Some(b.id.as_str()) != except && self.busy(&b.id)
            })
        })
    }
    fn command(&mut self, token: &str, b: &Value) -> Result<Value, String> {
        let (op, request_id) = crate::security::validate_request(b)?;
        if token.len() > 256 {
            return Err("invalid_credential".into());
        }
        self.expire_receipts();
        // A public request ID is not an authorization credential. Never cache the
        // one-time owner secret from anonymous registration for receipt replay.
        if op == "create_team" {
            return self.apply(token, op, b);
        }
        let key = format!("{}:{request_id}", hash(token));
        let payload = hash(&b.to_string());
        // Authenticate even cache hits: revoked credentials cannot retrieve prior receipts.
        if ["bid", "cash", "join", "leave", "auth"].contains(&op) {
            self.bot(token)?;
        }
        if [
            "create_bot",
            "rotate_bot",
            "rotate_owner",
            "account",
            "register_tournament",
            "unregister_tournament",
        ]
        .contains(&op)
        {
            self.owner(token)?;
        }
        if op.starts_with("admin_") {
            self.admin(token)?;
        }
        if let Some((old, result, _, _)) = self.retries.get(&key) {
            return if old == &payload {
                Ok(result.clone())
            } else {
                Err("request_id_reused".into())
            };
        }
        let result = self.apply(token, op, b)?;
        // Reads do not need receipt replay. Bound cached secrets by bytes and age.
        if !matches!(op, "auth" | "account") {
            let bytes = key.len() + payload.len() + result.to_string().len();
            if bytes <= 64 * 1024 {
                self.retry_bytes += bytes;
                self.retries.insert(
                    key.clone(),
                    (payload, result.clone(), Instant::now(), bytes),
                );
                self.retry_order.push_back(key);
                while self.retry_bytes > 4 * 1024 * 1024 || self.retry_order.len() > 2048 {
                    self.evict_receipt();
                }
            }
        }
        Ok(result)
    }
    fn evict_receipt(&mut self) {
        if let Some(key) = self.retry_order.pop_front()
            && let Some((_, _, _, bytes)) = self.retries.remove(&key)
        {
            self.retry_bytes -= bytes;
        }
    }
    fn expire_receipts(&mut self) {
        while self.retry_order.front().is_some_and(|key| {
            self.retries
                .get(key)
                .is_none_or(|(_, _, created, _)| created.elapsed() >= Duration::from_secs(120))
        }) {
            self.evict_receipt();
        }
    }
    fn apply(&mut self, token: &str, op: &str, b: &Value) -> Result<Value, String> {
        match op {
            "create_team" => {
                if self.world.teams.len() >= 10000 {
                    return Err("registration_full".into());
                }
                let name = label(b["name"].as_str().ok_or("missing_name")?)?;
                if self.indexes.team_names.contains(&name.to_lowercase()) {
                    return Err("team_name_taken".into());
                }
                let owner_token = secret();
                let t = Team {
                    id: id(),
                    name,
                    owner_hash: hash(&owner_token),
                    disabled: false,
                    created_ms: now_ms(),
                };
                self.world.teams.push(t.clone());
                self.persist(&[])?;
                self.broadcast_directory();
                Ok(json!({"team":{"id":t.id,"name":t.name},"owner_token":owner_token}))
            }
            "account" => {
                let team = self.owner(token)?;
                let t = self.world.teams.iter().find(|t| t.id == team).unwrap();
                Ok(
                    json!({"team":{"id":t.id,"name":t.name},"bots":self.world.bots.iter().filter(|b|b.team_id==team).map(|b|self.bot_public(b)).collect::<Vec<_>>()}),
                )
            }
            "create_bot" => {
                let team = self.owner(token)?;
                if self.world.bots.len() >= 10000
                    || self
                        .indexes
                        .team_bots
                        .get(&team)
                        .is_some_and(|bots| bots.len() >= 100)
                {
                    return Err("version_limit".into());
                }
                let name = label(b["name"].as_str().ok_or("missing_name")?)?;
                if self
                    .world
                    .bots
                    .iter()
                    .any(|b| b.team_id == team && b.name == name)
                {
                    return Err("version_name_taken".into());
                }
                let bot_token = secret();
                let bot = Bot {
                    id: id(),
                    team_id: team,
                    name,
                    token_hash: hash(&bot_token),
                    rating: 1500.,
                    games: 0,
                    disabled: false,
                    created_ms: now_ms(),
                };
                self.world.bots.push(bot.clone());
                self.persist(&[])?;
                self.broadcast_directory();
                Ok(json!({"bot":self.bot_public(&bot),"bot_token":bot_token}))
            }
            "rotate_bot" => {
                let team = self.owner(token)?;
                let bot_id = b["bot_id"].as_str().ok_or("missing_bot_id")?;
                let bot = self
                    .world
                    .bots
                    .iter_mut()
                    .find(|x| x.id == bot_id && x.team_id == team)
                    .ok_or("bot_not_found")?;
                let token = secret();
                bot.token_hash = hash(&token);
                self.persist(&[])?;
                Ok(json!({"bot_token":token}))
            }
            "rotate_owner" => {
                let team = self.owner(token)?;
                let token = secret();
                self.world
                    .teams
                    .iter_mut()
                    .find(|t| t.id == team)
                    .unwrap()
                    .owner_hash = hash(&token);
                self.persist(&[])?;
                Ok(json!({"owner_token":token}))
            }
            "auth" => {
                let bot = self.bot(token)?;
                Ok(
                    json!({"bot_id":bot,"active_match":self.live.iter().find(|(_,l)|l.game.players.iter().any(|p|p.id==bot)).map(|(id,_)|id)}),
                )
            }
            "join" => {
                if self.world.paused {
                    return Err("queue_paused".into());
                }
                let bot = self.bot(token)?;
                if self.busy(&bot) {
                    return Err("already_active".into());
                }
                let team = &self
                    .world
                    .bots
                    .iter()
                    .find(|b| b.id == bot)
                    .unwrap()
                    .team_id;
                if self.team_busy(team, Some(&bot)) {
                    return Err("team_already_active".into());
                }
                if self.queue.len() >= 512 {
                    return Err("queue_full".into());
                }
                self.queue.push_back(bot);
                self.broadcast_directory();
                Ok(json!({"queued":true}))
            }
            "leave" => {
                let bot = self.bot(token)?;
                self.queue.retain(|x| x != &bot);
                self.broadcast_directory();
                Ok(json!({"queued":false}))
            }
            "bid" | "cash" => {
                let bot = self.bot(token)?;
                let mid = b["match_id"].as_str().ok_or("missing_match_id")?;
                let round = b["round"].as_u64().ok_or("missing_round")? as usize;
                let l = self.live.get_mut(mid).ok_or("match_not_live")?;
                if Instant::now() >= l.deadline {
                    return Err("deadline_passed".into());
                }
                let seat = l
                    .game
                    .players
                    .iter()
                    .position(|p| p.id == bot)
                    .ok_or("not_seated")?;
                if op == "bid" {
                    let amount = u32::try_from(b["amount"].as_u64().ok_or("invalid_amount")?)
                        .map_err(|_| "invalid_amount")?;
                    l.game.bid(seat, round, amount).map_err(|e| e.to_string())?;
                    Ok(json!({"accepted":true,"round":round}))
                } else {
                    let cards: Vec<u8> =
                        serde_json::from_value(b["cards"].clone()).map_err(|_| "invalid_cards")?;
                    let cards: [u8; 5] = cards.try_into().map_err(|_| "invalid_cards")?;
                    l.game.cash(seat, round, cards).map_err(|e| e.to_string())?;
                    l.history.push(l.game.snapshot());
                    let record = l.record(mid, "live");
                    let public = l.public(mid);
                    let bots = l.game.players.iter().map(|p| p.id.clone()).collect();
                    self.persist_records(&[record])?;
                    self.live.get_mut(mid).unwrap().mark_persisted();
                    self.publish(
                        &format!("match:{mid}"),
                        bots,
                        json!({"type":"match","match":public}),
                    );
                    Ok(json!({"accepted":true}))
                }
            }
            "register_tournament" => {
                let team = self.owner(token)?;
                let bot_id = b["bot_id"].as_str().ok_or("missing_bot_id")?;
                let bot = self
                    .world
                    .bots
                    .iter()
                    .find(|x| x.id == bot_id && x.team_id == team && !x.disabled)
                    .ok_or("bot_not_found")?;
                if self.busy(bot_id) || self.team_busy(&team, Some(bot_id)) {
                    return Err("team_already_active".into());
                }
                let bot_id = bot.id.clone();
                let tid = b["tournament_id"].as_str().ok_or("missing_tournament_id")?;
                let t = self
                    .world
                    .tournaments
                    .iter_mut()
                    .find(|t| t.id == tid)
                    .ok_or("tournament_not_found")?;
                t.register(bot_id)?;
                self.persist(&[])?;
                self.broadcast_directory();
                Ok(json!({"registered":true}))
            }
            "unregister_tournament" => {
                let team = self.owner(token)?;
                let bot_id = b["bot_id"].as_str().ok_or("missing_bot_id")?;
                if !self
                    .world
                    .bots
                    .iter()
                    .any(|x| x.id == bot_id && x.team_id == team)
                {
                    return Err("bot_not_found".into());
                }
                let tid = b["tournament_id"].as_str().ok_or("missing_tournament_id")?;
                let t = self
                    .world
                    .tournaments
                    .iter_mut()
                    .find(|t| t.id == tid)
                    .ok_or("tournament_not_found")?;
                if t.state != "registration" {
                    return Err("registration_closed".into());
                }
                t.entrants.retain(|e| e.bot != bot_id);
                self.persist(&[])?;
                Ok(json!({"registered":false}))
            }
            "admin_create_tournament" => {
                self.admin(token)?;
                if self.world.tournaments.len() >= 256 {
                    return Err("tournament_limit".into());
                }
                let name = label(b["name"].as_str().ok_or("missing_name")?)?;
                let rounds = b["swiss_rounds"].as_u64().ok_or("missing_swiss_rounds")? as usize;
                let cut = b["cut"].as_u64().ok_or("missing_cut")? as usize;
                let t = Tournament::new(id(), name, rounds, cut)?;
                self.world.tournaments.push(t.clone());
                self.persist(&[])?;
                Ok(json!({"tournament":t}))
            }
            "admin_start_tournament" => {
                self.admin(token)?;
                let tid = b["tournament_id"].as_str().ok_or("missing_tournament_id")?;
                let t = self
                    .world
                    .tournaments
                    .iter_mut()
                    .find(|t| t.id == tid)
                    .ok_or("tournament_not_found")?;
                t.start()?;
                self.persist(&[])?;
                Ok(json!({"started":true}))
            }
            "admin_cancel_tournament" => {
                self.admin(token)?;
                let tid = b["tournament_id"].as_str().ok_or("missing_tournament_id")?;
                let t = self
                    .world
                    .tournaments
                    .iter_mut()
                    .find(|t| t.id == tid)
                    .ok_or("tournament_not_found")?;
                if t.state == "complete" {
                    return Err("already_complete".into());
                }
                t.state = "cancelled".into();
                let ids: Vec<_> = self
                    .live
                    .iter()
                    .filter(|(_, l)| l.tournament.as_ref().is_some_and(|p| p.0 == tid))
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ids {
                    self.abort_match(&id, "tournament_cancelled")?;
                }
                self.persist(&[])?;
                self.broadcast_directory();
                Ok(json!({"cancelled":true}))
            }
            "admin_pause" => {
                self.admin(token)?;
                self.world.paused = b["paused"].as_bool().ok_or("missing_paused")?;
                self.persist(&[])?;
                self.broadcast_directory();
                Ok(json!({"paused":self.world.paused}))
            }
            "admin_abort_match" => {
                self.admin(token)?;
                let mid = b["match_id"].as_str().ok_or("missing_match_id")?;
                self.abort_match(mid, "operator_abort")?;
                self.broadcast_directory();
                Ok(json!({"aborted":true}))
            }
            "admin_disable_team" => {
                self.admin(token)?;
                let tid = b["team_id"].as_str().ok_or("missing_team_id")?;
                let disabled = b["disabled"].as_bool().ok_or("missing_disabled")?;
                self.world
                    .teams
                    .iter_mut()
                    .find(|t| t.id == tid)
                    .ok_or("team_not_found")?
                    .disabled = disabled;
                let ids: HashSet<_> = self
                    .world
                    .bots
                    .iter()
                    .filter(|b| b.team_id == tid)
                    .map(|b| b.id.clone())
                    .collect();
                if disabled {
                    self.queue.retain(|b| !ids.contains(b));
                    let mids: Vec<_> = self
                        .live
                        .iter()
                        .filter(|(_, l)| l.game.players.iter().any(|p| ids.contains(&p.id)))
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in mids {
                        self.abort_match(&id, "team_disabled")?;
                    }
                    // Cancel every pairing of affected tournaments, including other teams' live games.
                    let mut cancelled = HashSet::new();
                    for t in &mut self.world.tournaments {
                        if t.entrants.iter().any(|e| ids.contains(&e.bot))
                            && !["complete", "cancelled"].contains(&t.state.as_str())
                        {
                            t.state = "cancelled".into();
                            cancelled.insert(t.id.clone());
                        }
                    }
                    let mids: Vec<_> = self
                        .live
                        .iter()
                        .filter(|(_, l)| {
                            l.tournament
                                .as_ref()
                                .is_some_and(|p| cancelled.contains(&p.0))
                        })
                        .map(|(id, _)| id.clone())
                        .collect();
                    for mid in mids {
                        self.abort_match(&mid, "tournament_cancelled")?;
                    }
                }
                self.persist(&[])?;
                self.broadcast_directory();
                Ok(json!({"disabled":disabled}))
            }
            _ => Err("unknown_operation".into()),
        }
    }
    fn start_match(
        &mut self,
        ids: Vec<String>,
        tournament: Option<(String, usize)>,
        demo: bool,
    ) -> Result<(), String> {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        self.start_match_seeded(ids, tournament, demo, seed)
    }
    fn start_match_seeded(
        &mut self,
        ids: Vec<String>,
        tournament: Option<(String, usize)>,
        demo: bool,
        seed: [u8; 32],
    ) -> Result<(), String> {
        let mut players: Vec<_> = ids
            .iter()
            .map(|id| {
                let b = self.world.bots.iter().find(|b| &b.id == id).unwrap();
                let t = self.world.teams.iter().find(|t| t.id == b.team_id).unwrap();
                Player::new(b.id.clone(), format!("{} / {}", t.name, b.name), false)
            })
            .collect();
        let seats = if tournament.is_some() {
            2
        } else {
            self.rules.seats
        };
        for i in players.len()..seats {
            players.push(Player::new(
                format!("house-{i}"),
                [
                    "The Combinator",
                    "Value Engine",
                    "River Rat",
                    "Cold Deck",
                    "Pocket Rocket",
                    "The Optimizer",
                    "Chip Circuit",
                    "Ace Agent",
                ][i]
                    .into(),
                true,
            ));
        }
        let game = Game::new(
            Rules {
                seats,
                ..self.rules.clone()
            },
            players,
            seed,
        )
        .map_err(|e| e.to_string())?;
        let mid = id();
        if let Some((tid, ix)) = &tournament {
            self.world
                .tournaments
                .iter_mut()
                .find(|t| &t.id == tid)
                .unwrap()
                .pairings[*ix]
                .match_id = Some(mid.clone());
        }
        let snap = game.snapshot();
        let mut l = Live {
            game,
            seed: hex::encode(seed),
            created_ms: now_ms(),
            deadline: Instant::now() + Duration::from_millis(self.rules.auction_ms),
            deadline_ms: now_ms() + self.rules.auction_ms,
            history: vec![snap],
            tournament,
            saved_events: 0,
            saved_history: 0,
        };
        let record = l.record(&mid, "live");
        if l.tournament.is_some() {
            self.persist(&[record])?;
        } else {
            self.persist_records(&[record])?;
        }
        l.mark_persisted();
        l.deadline = Instant::now() + Duration::from_millis(self.rules.auction_ms);
        l.deadline_ms = now_ms() + self.rules.auction_ms;
        let public = l.public(&mid);
        let bots = l.game.players.iter().map(|p| p.id.clone()).collect();
        self.live.insert(mid.clone(), l);
        self.publish(
            &format!("match:{mid}"),
            bots,
            json!({"type":"match","match":public,"demo":demo}),
        );
        self.broadcast_directory();
        Ok(())
    }
    fn tick(&mut self) -> Result<(), String> {
        self.expire_receipts();
        let due: Vec<_> = self
            .live
            .iter()
            .filter(|(_, l)| Instant::now() >= l.deadline)
            .map(|(id, _)| id.clone())
            .collect();
        for mid in due {
            let Some(l) = self.live.get_mut(&mid) else {
                continue;
            };
            self.max_tick_lag_ms = self.max_tick_lag_ms.max(
                Instant::now()
                    .saturating_duration_since(l.deadline)
                    .as_millis() as u64,
            );
            for seat in 0..l.game.players.len() {
                if l.game.players[seat].house {
                    play_house(&mut l.game, seat);
                }
            }
            l.game.settle().map_err(|e| e.to_string())?;
            l.history.push(l.game.snapshot());
            if l.game.snapshot().finished {
                self.complete(&mid)?;
            } else {
                // A fresh full window begins after durable settlement; never catch up by skipping windows.
                let record = l.record(&mid, "live");
                self.persist_records(&[record])?;
                let l = self.live.get_mut(&mid).unwrap();
                l.mark_persisted();
                l.deadline = Instant::now() + Duration::from_millis(l.game.rules.auction_ms);
                l.deadline_ms = now_ms() + l.game.rules.auction_ms;
                let public = l.public(&mid);
                let bots = l.game.players.iter().map(|p| p.id.clone()).collect();
                self.publish(
                    &format!("match:{mid}"),
                    bots,
                    json!({"type":"match","match":public}),
                );
            }
        }
        if !self.world.paused {
            let pending: Vec<_> = self
                .world
                .tournaments
                .iter()
                .filter(|t| ["swiss", "elimination", "final"].contains(&t.state.as_str()))
                .flat_map(|t| {
                    t.pairings
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.winner.is_none() && p.match_id.is_none())
                        .map(move |(ix, p)| {
                            (t.id.clone(), ix, vec![p.a.clone(), p.b.clone().unwrap()])
                        })
                })
                .collect();
            for (tid, ix, ids) in pending {
                if self.live.len() >= self.max_live {
                    break;
                }
                self.start_match(ids, Some((tid, ix)), false)?;
            }
            if self.live.len() < self.max_live
                && !self.queue.is_empty()
                && (self.queue.len() >= self.rules.seats || Instant::now() >= self.lobby_deadline)
            {
                let ids: Vec<_> = (0..self.queue.len().min(self.rules.seats))
                    .filter_map(|_| self.queue.pop_front())
                    .collect();
                self.start_match(ids, None, false)?;
                self.reset_lobby();
            }
            if self.demo && self.live.is_empty() && self.queue.is_empty() {
                self.start_match(vec![], None, true)?;
            }
        }
        if Instant::now() >= self.lobby_deadline && self.queue.is_empty() {
            self.reset_lobby();
            self.broadcast_directory();
        }
        Ok(())
    }
    fn reset_lobby(&mut self) {
        self.lobby_deadline = Instant::now() + Duration::from_secs(30);
        self.lobby_deadline_ms = now_ms() + 30000;
    }
    fn complete(&mut self, mid: &str) -> Result<(), String> {
        let l = self.live.remove(mid).unwrap();
        let real: Vec<_> = l.game.players.iter().filter(|p| !p.house).collect();
        // Rate real competitors only; padding cannot farm rating.
        if real.len() >= 2 {
            let ratings: Vec<_> = real
                .iter()
                .map(|p| {
                    self.world
                        .bots
                        .iter()
                        .find(|b| b.id == p.id)
                        .unwrap()
                        .rating
                })
                .collect();
            let scores: Vec<_> = real.iter().map(|p| p.score).collect();
            let updated = elo_updates(&ratings, &scores);
            for (i, p) in real.iter().enumerate() {
                let b = self.world.bots.iter_mut().find(|b| b.id == p.id).unwrap();
                b.rating = updated[i];
                b.games += 1;
            }
        }
        let mut suspended = None;
        if let Some((tid, ix)) = &l.tournament {
            let winner = if real[0].score == real[1].score {
                None
            } else {
                Some(if real[0].score > real[1].score {
                    real[0].id.clone()
                } else {
                    real[1].id.clone()
                })
            };
            if let Some(t) = self.world.tournaments.iter_mut().find(|t| &t.id == tid)
                && t.state != "cancelled"
            {
                t.result(*ix, winner)?;
                if t.state == "attention" {
                    suspended = Some(tid.clone());
                }
            }
        }
        self.persist(&[l.record(mid, "complete")])?;
        if let Some(tid) = suspended {
            let others: Vec<_> = self
                .live
                .iter()
                .filter(|(_, game)| game.tournament.as_ref().is_some_and(|t| t.0 == tid))
                .map(|(id, _)| id.clone())
                .collect();
            for id in others {
                self.abort_match(&id, "tournament_suspended")?;
            }
        }
        let mut public = l.public(mid);
        public["status"] = json!("complete");
        let bots = l.game.players.iter().map(|p| p.id.clone()).collect();
        self.publish(
            &format!("match:{mid}"),
            bots,
            json!({"type":"match","match":public}),
        );
        self.broadcast_directory();
        Ok(())
    }
    fn abort_match(&mut self, mid: &str, reason: &str) -> Result<(), String> {
        let mut l = self.live.remove(mid).ok_or("match_not_live")?;
        l.game.abort(reason);
        l.history.push(l.game.snapshot());
        if let Some((tid, ix)) = &l.tournament
            && let Some(t) = self.world.tournaments.iter_mut().find(|t| &t.id == tid)
        {
            t.pairings[*ix].match_id = None;
        }
        self.persist(&[l.record(mid, "aborted")])?;
        let mut public = l.public(mid);
        public["status"] = json!("aborted");
        let bots = l.game.players.iter().map(|p| p.id.clone()).collect();
        self.publish(
            &format!("match:{mid}"),
            bots,
            json!({"type":"match","match":public}),
        );
        Ok(())
    }
    fn abort_all(&mut self, reason: &str) -> Result<(), String> {
        let ids: Vec<_> = self.live.keys().cloned().collect();
        for id in ids {
            self.abort_match(&id, reason)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service() -> Service {
        let (feed, _) = broadcast::channel(32);
        Service {
            store: Store::open(":memory:").unwrap(),
            world: World::default(),
            indexes: Indexes::default(),
            live: HashMap::new(),
            queue: VecDeque::new(),
            lobby_deadline: Instant::now() + Duration::from_secs(30),
            lobby_deadline_ms: now_ms() + 30000,
            feed,
            admin_hash: Some(hash("test-admin")),
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
        }
    }
    fn cmd(s: &mut Service, token: &str, mut body: Value) -> Result<Value, String> {
        if body.get("id").is_none() {
            body["id"] = json!(id());
        }
        s.command(token, &body)
    }
    fn user(s: &mut Service, name: &str) -> (String, String, String) {
        let t = cmd(s, "", json!({"op":"create_team","name":name})).unwrap();
        let owner = t["owner_token"].as_str().unwrap().to_owned();
        let b = cmd(s, &owner, json!({"op":"create_bot","name":"v1"})).unwrap();
        (
            owner,
            b["bot_token"].as_str().unwrap().into(),
            b["bot"]["id"].as_str().unwrap().into(),
        )
    }
    #[test]
    fn anonymous_registration_cannot_replay_owner_secret() {
        let mut s = service();
        let body = json!({"op":"create_team","id":"predictable-public-id","name":"Alpha"});
        let created = s.command("", &body).unwrap();
        assert!(created["owner_token"].is_string());
        assert_eq!(s.command("", &body).unwrap_err(), "team_name_taken");
        assert!(s.retries.is_empty());
    }
    #[test]
    fn credentials_scopes_rotation_and_no_leaks() {
        let mut s = service();
        let (owner, token, bot) = user(&mut s, "Alpha");
        assert!(cmd(&mut s, &token, json!({"op":"create_bot","name":"v2"})).is_err());
        assert!(cmd(&mut s, &owner, json!({"op":"join"})).is_err());
        let public = s.read("bots", "", 0).unwrap().to_string();
        assert!(!public.contains(&token) && !public.contains("hash"));
        cmd(&mut s, &owner, json!({"op":"rotate_bot","bot_id":bot})).unwrap();
        assert!(cmd(&mut s, &token, json!({"op":"auth"})).is_err());
        assert!(!s.store.world().unwrap().bots[0].token_hash.contains(&token));
    }
    #[test]
    fn immutable_bids_retry_privacy_and_deadlines() {
        let mut s = service();
        let (_, token, bot) = user(&mut s, "Alpha");
        s.start_match(vec![bot.clone()], None, false).unwrap();
        let mid = s.live.keys().next().unwrap().clone();
        let b = json!({"op":"bid","id":"same","match_id":mid,"round":0,"amount":13});
        let before = s.read("match", &mid, 0).unwrap()["snapshot"].clone();
        let receipt = s.command(&token, &b).unwrap();
        assert_eq!(receipt, s.command(&token, &b).unwrap());
        let mut changed = b.clone();
        changed["amount"] = json!(14);
        assert_eq!(
            s.command(&token, &changed).unwrap_err(),
            "request_id_reused"
        );
        assert_eq!(before, s.read("match", &mid, 0).unwrap()["snapshot"]);
        let replay = s.read("replay", &mid, 0).unwrap().to_string();
        assert!(!replay.contains("private_seed") && !replay.contains("bids"));
        s.live.get_mut(&mid).unwrap().deadline = Instant::now();
        assert_eq!(
            cmd(
                &mut s,
                &token,
                json!({"op":"bid","match_id":mid,"round":0,"amount":12})
            )
            .unwrap_err(),
            "deadline_passed"
        );
    }
    #[test]
    fn team_queue_and_tournament_exclusivity() {
        let mut s = service();
        let (owner, token, bot) = user(&mut s, "Alpha");
        let b = cmd(&mut s, &owner, json!({"op":"create_bot","name":"v2"})).unwrap();
        cmd(&mut s, &token, json!({"op":"join"})).unwrap();
        assert_eq!(
            cmd(
                &mut s,
                b["bot_token"].as_str().unwrap(),
                json!({"op":"join"})
            )
            .unwrap_err(),
            "team_already_active"
        );
        let t = cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_create_tournament","name":"Cup","cut":2,"swiss_rounds":1}),
        )
        .unwrap();
        assert!(cmd(&mut s,&owner,json!({"op":"register_tournament","tournament_id":t["tournament"]["id"],"bot_id":bot})).is_err());
    }
    #[test]
    fn restart_aborts_and_preserves_rating() {
        let mut s = service();
        let (_, _, bot) = user(&mut s, "Alpha");
        s.start_match(vec![bot], None, false).unwrap();
        let mid = s.live.keys().next().unwrap().clone();
        let mut w = s.store.world().unwrap();
        assert_eq!(s.store.recover(&mut w).unwrap(), 1);
        let r = s.store.record(&mid).unwrap();
        assert_eq!(r["status"], "aborted");
        assert!(r["snapshot"]["seed"].is_string());
        assert_eq!(w.bots[0].games, 0);
        assert_eq!(s.store.recover(&mut w).unwrap(), 0);
    }
    #[test]
    fn completed_result_and_ratings_are_atomic_and_persistent() {
        let mut s = service();
        let (_, _, a) = user(&mut s, "Alpha");
        let (_, _, b) = user(&mut s, "Bravo");
        s.start_match(vec![a, b], None, false).unwrap();
        let mid = s.live.keys().next().unwrap().clone();
        for _ in 0..52 {
            let l = s.live.get_mut(&mid).unwrap();
            for seat in 0..2 {
                play_house(&mut l.game, seat);
            }
            l.game.settle().unwrap();
        }
        s.complete(&mid).unwrap();
        assert_eq!(s.store.record(&mid).unwrap()["status"], "complete");
        let w = s.store.world().unwrap();
        assert!(w.bots.iter().all(|b| b.games == 1));
        assert!((w.bots.iter().map(|b| b.rating).sum::<f64>() - 3000.).abs() < 1e-8);
        assert!(s.live.is_empty());
    }
    #[test]
    fn disabling_team_cancels_all_active_tournament_pairings() {
        let mut s = service();
        let t = cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_create_tournament","name":"Four Cup","cut":4,"swiss_rounds":1}),
        )
        .unwrap();
        let tid = t["tournament"]["id"].clone();
        let mut team = String::new();
        for name in ["Alpha", "Bravo", "Charlie", "Delta"] {
            let (owner, _, bot) = user(&mut s, name);
            if team.is_empty() {
                team = s.owner(&owner).unwrap();
            }
            cmd(
                &mut s,
                &owner,
                json!({"op":"register_tournament","tournament_id":tid,"bot_id":bot}),
            )
            .unwrap();
        }
        cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_start_tournament","tournament_id":tid}),
        )
        .unwrap();
        s.tick().unwrap();
        assert_eq!(s.live.len(), 2);
        cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_disable_team","team_id":team,"disabled":true}),
        )
        .unwrap();
        assert!(s.live.is_empty());
        assert_eq!(s.world.tournaments[0].state, "cancelled");
        assert!(s.world.bots.iter().all(|b| b.games == 0));
    }
    #[test]
    fn public_bot_and_team_history_are_scoped() {
        let mut s = service();
        let (owner, _, bot) = user(&mut s, "Alpha");
        let (_, _, other) = user(&mut s, "Bravo");
        let team = s.owner(&owner).unwrap();
        s.start_match(vec![bot.clone()], None, false).unwrap();
        assert_eq!(
            s.read("bot_matches", &bot, 0).unwrap()["matches"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            s.read("team_matches", &team, 0).unwrap()["matches"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            s.read("bot_matches", &other, 0).unwrap()["matches"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(s.read("bot_matches", "missing", 0).is_err());
    }
    #[test]
    fn receipts_expire_and_read_results_do_not_accumulate() {
        let mut s = service();
        let (owner, token, _) = user(&mut s, "Alpha");
        let before = s.retries.len();
        for _ in 0..20 {
            cmd(&mut s, &owner, json!({"op":"account"})).unwrap();
            cmd(&mut s, &token, json!({"op":"auth"})).unwrap();
        }
        assert_eq!(s.retries.len(), before);
        for (_, _, created, _) in s.retries.values_mut() {
            *created = Instant::now() - Duration::from_secs(121);
        }
        s.expire_receipts();
        assert!(s.retries.is_empty());
        assert_eq!(s.retry_bytes, 0);
        assert!(label("bad\u{202e}name").is_err());
    }
    #[test]
    fn incremental_round_commits_reconstruct_a_complete_verified_replay() {
        let mut s = service();
        let (_, _, a) = user(&mut s, "Alpha");
        let (_, _, b) = user(&mut s, "Bravo");
        s.start_match(vec![a, b], None, false).unwrap();
        let mid = s.live.keys().next().unwrap().clone();
        for _ in 0..52 {
            s.live.get_mut(&mid).unwrap().deadline = Instant::now();
            s.tick().unwrap();
        }
        let r = s.store.record(&mid).unwrap();
        assert_eq!(r["history"].as_array().unwrap().len(), 53);
        assert_eq!(r["events"].as_array().unwrap().len(), 58);
        assert!(pokerdraft_engine::replay::verify(r).is_ok());
    }
    #[test]
    fn drawn_tournament_suspension_aborts_other_pairings_without_rating_them() {
        let mut s = service();
        let cup = cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_create_tournament","name":"Draw Cup","cut":4,"swiss_rounds":1}),
        )
        .unwrap();
        let tid = cup["tournament"]["id"].clone();
        for name in ["Alpha", "Bravo", "Charlie", "Delta"] {
            let (owner, _, bot) = user(&mut s, name);
            cmd(
                &mut s,
                &owner,
                json!({"op":"register_tournament","tournament_id":tid,"bot_id":bot}),
            )
            .unwrap();
        }
        cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_start_tournament","tournament_id":tid}),
        )
        .unwrap();
        s.tick().unwrap();
        let pair = 0;
        let mut target = String::new();
        for draw in 0..3 {
            let mid = s.world.tournaments[0].pairings[pair]
                .match_id
                .clone()
                .unwrap();
            target = mid.clone();
            let l = s.live.get_mut(&mid).unwrap();
            while !l.game.snapshot().finished {
                l.game.settle().unwrap();
                l.history.push(l.game.snapshot());
            }
            s.complete(&mid).unwrap();
            if draw < 2 {
                s.tick().unwrap();
            }
        }
        assert_eq!(s.world.tournaments[0].state, "attention");
        assert!(s.live.is_empty());
        assert!(s.world.tournaments[0].champion.is_none());
        let records = s.store.recent(20, 0).unwrap();
        assert_eq!(
            records.iter().filter(|r| r["status"] == "aborted").count(),
            1
        );
        assert_eq!(
            records.iter().filter(|r| r["status"] == "complete").count(),
            3
        );
        assert!(pokerdraft_engine::replay::verify(s.store.record(&target).unwrap()).is_ok());
        assert!(s.read("health", "", 0).unwrap().get("commands").is_none());
    }
    #[test]
    fn ratings_preserve_exact_bits_across_durable_reload() {
        let mut s = service();
        user(&mut s, "Alpha");
        user(&mut s, "Bravo");
        let mut ratings = vec![1500., 1500.];
        for i in 0..128 {
            ratings = elo_updates(&ratings, if i % 3 == 0 { &[1, 0] } else { &[0, 1] });
            for (bot, &rating) in s.world.bots.iter_mut().zip(&ratings) {
                bot.rating = rating;
            }
            s.persist(&[]).unwrap();
            let loaded = s.store.world().unwrap();
            for (bot, &rating) in loaded.bots.iter().zip(&ratings) {
                assert_eq!(
                    bot.rating.to_bits(),
                    rating.to_bits(),
                    "rating changed across JSON storage: {} -> {}",
                    rating,
                    bot.rating
                );
            }
        }
    }
    #[test]
    fn admin_authorization_required() {
        let mut s = service();
        assert_eq!(
            cmd(&mut s, "", json!({"op":"admin_pause","paused":true})).unwrap_err(),
            "admin_auth_required"
        );
        assert!(!s.world.paused);
        cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_pause","paused":true}),
        )
        .unwrap();
        assert!(s.store.world().unwrap().paused);
    }
    #[test]
    fn tournament_recovers_inflight_pairings() {
        let mut s = service();
        let (oa, _, a) = user(&mut s, "Alpha");
        let (ob, _, b) = user(&mut s, "Bravo");
        let t = cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_create_tournament","name":"Cup","cut":2,"swiss_rounds":1}),
        )
        .unwrap();
        let tid = t["tournament"]["id"].clone();
        for (o, bot) in [(oa, a), (ob, b)] {
            cmd(
                &mut s,
                &o,
                json!({"op":"register_tournament","tournament_id":tid,"bot_id":bot}),
            )
            .unwrap();
        }
        cmd(
            &mut s,
            "test-admin",
            json!({"op":"admin_start_tournament","tournament_id":tid}),
        )
        .unwrap();
        s.tick().unwrap();
        assert_eq!(s.live.len(), 1);
        let mut w = s.store.world().unwrap();
        assert!(w.tournaments[0].pairings[0].match_id.is_some());
        s.store.recover(&mut w).unwrap();
        assert!(w.tournaments[0].pairings[0].match_id.is_none());
        assert_eq!(w.tournaments[0].state, "swiss");
    }
}

#[cfg(feature = "profiling")]
#[path = "profile.rs"]
pub mod profile;

#[cfg(test)]
mod optimization_tests {
    use super::*;
    #[test]
    fn public_frames_share_serialized_bytes() {
        let f = Feed::new(
            "directory",
            vec!["public-id".into()],
            json!({"public":true}),
        );
        let copy = f.clone();
        assert_eq!(f.value.as_str().as_ptr(), copy.value.as_str().as_ptr());
        assert!(std::sync::Arc::ptr_eq(&f.bots, &copy.bots));
        assert_eq!(
            serde_json::from_str::<Value>(f.value.as_str()).unwrap(),
            json!({"public":true})
        );
    }
}
