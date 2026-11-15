use crate::service::World;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs::File,
    time::{Duration, Instant},
};

/// One leased writer, plus explicitly read-only archive connections.
pub struct Store {
    pub conn: Connection,
    _lease: Option<File>,
    indexed_bots: HashSet<String>,
    indexed_teams: HashSet<String>,
    public_cache: Option<PublicCache>,
}
// Deserialize only public fields; credential hashes never enter the read cache.
#[derive(Default, serde::Deserialize)]
struct PublicWorld {
    teams: Vec<PublicTeam>,
    bots: Vec<PublicBot>,
    tournaments: Vec<pokerdraft_engine::tournament::Tournament>,
}
#[derive(serde::Deserialize)]
struct PublicTeam {
    id: String,
    name: String,
    disabled: bool,
}
#[derive(serde::Deserialize)]
struct PublicBot {
    id: String,
    team_id: String,
    name: String,
    rating: f64,
    games: u32,
    disabled: bool,
    created_ms: u64,
}
struct PublicCache {
    revision: i64,
    world: PublicWorld,
    teams: std::collections::HashMap<String, usize>,
    bots: std::collections::HashMap<String, usize>,
    ranked: Vec<usize>,
}
impl PublicCache {
    fn new(revision: i64, world: PublicWorld) -> Self {
        let teams: std::collections::HashMap<_, _> = world
            .teams
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        let bots = world
            .bots
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id.clone(), i))
            .collect();
        let mut ranked: Vec<_> = world
            .bots
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                !b.disabled
                    && b.games > 0
                    && teams
                        .get(&b.team_id)
                        .is_some_and(|&i| !world.teams[i].disabled)
            })
            .map(|(i, _)| i)
            .collect();
        ranked.sort_by(|&a, &b| world.bots[b].rating.total_cmp(&world.bots[a].rating));
        Self {
            revision,
            world,
            teams,
            bots,
            ranked,
        }
    }
    fn bot(&self, i: usize) -> Value {
        let b = &self.world.bots[i];
        let team = self.teams.get(&b.team_id).map(|&i| &self.world.teams[i]);
        json!({"id":b.id,"team_id":b.team_id,"team":team.map(|t|&t.name),"name":b.name,"rating":b.rating,"games":b.games,"disabled":b.disabled||team.is_some_and(|t|t.disabled),"created_ms":b.created_ms})
    }
    fn name(&self, id: &str) -> String {
        self.bots
            .get(id)
            .map(|&i| {
                let b = &self.world.bots[i];
                format!(
                    "{} / {}",
                    self.teams
                        .get(&b.team_id)
                        .map(|&j| self.world.teams[j].name.as_str())
                        .unwrap_or("Unknown team"),
                    b.name
                )
            })
            .unwrap_or_else(|| id.to_owned())
    }
}
fn sql_error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn parse(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Value> {
    let s: String = row.get(i)?;
    serde_json::from_str(&s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let s = parse(row, 3)?;
    Ok(
        json!({"id":row.get::<_,String>(0)?,"status":row.get::<_,String>(1)?,"created_ms":row.get::<_,i64>(2)?,"players":s["players"],"round":s["round"],"tournament":row.get::<_,Option<String>>(4)?}),
    )
}
impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let lease = if path == ":memory:" {
            None
        } else {
            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options
                .open(path)
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            file.try_lock().map_err(|_| {
                rusqlite::Error::InvalidParameterName("database_already_in_use".into())
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            }
            Some(file)
        };
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let has_version: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='migrations')",
            [],
            |r| r.get(0),
        )?;
        let version: i64 = if has_version {
            conn.query_row("SELECT coalesce(max(version),0) FROM migrations", [], |r| {
                r.get(0)
            })?
        } else {
            0
        };
        if version > 2 {
            return Err(rusqlite::Error::InvalidParameterName(
                "unsupported_database_version".into(),
            ));
        }
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
   CREATE TABLE IF NOT EXISTS migrations(version INTEGER PRIMARY KEY);
   CREATE TABLE IF NOT EXISTS world(id INTEGER PRIMARY KEY CHECK(id=1),data TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS matches(id TEXT PRIMARY KEY,status TEXT NOT NULL,created_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL,snapshot TEXT NOT NULL,events TEXT NOT NULL,history TEXT NOT NULL,seed TEXT NOT NULL,tournament TEXT);")?;
        if version < 2 {
            let raw: Option<String> = conn
                .query_row("SELECT data FROM world WHERE id=1", [], |r| r.get(0))
                .optional()?;
            let world: World = raw
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?
                .unwrap_or_default();
            let tx = conn.transaction()?;
            tx.execute_batch("CREATE INDEX IF NOT EXISTS match_recent_v2 ON matches(created_ms DESC,id);
    CREATE TABLE IF NOT EXISTS team_ids(id TEXT PRIMARY KEY);
    CREATE TABLE IF NOT EXISTS metadata_revision(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL);
    INSERT OR IGNORE INTO metadata_revision VALUES(1,0);
    CREATE TABLE IF NOT EXISTS bot_teams(bot_id TEXT PRIMARY KEY,team_id TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS team_bots_lookup ON bot_teams(team_id,bot_id);
    CREATE TABLE IF NOT EXISTS match_players(match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,bot_id TEXT NOT NULL,team_id TEXT NOT NULL,created_ms INTEGER NOT NULL,PRIMARY KEY(match_id,bot_id));
    CREATE INDEX IF NOT EXISTS player_history ON match_players(bot_id,created_ms DESC,match_id);
    CREATE INDEX IF NOT EXISTS team_history ON match_players(team_id,created_ms DESC,match_id);
    CREATE UNIQUE INDEX IF NOT EXISTS one_team_per_match ON match_players(match_id,team_id) WHERE team_id<>'';
    CREATE TABLE IF NOT EXISTS match_events(match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,seq INTEGER NOT NULL,data TEXT NOT NULL,PRIMARY KEY(match_id,seq));
    CREATE TABLE IF NOT EXISTS match_history(match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,position INTEGER NOT NULL,data TEXT NOT NULL,PRIMARY KEY(match_id,position));")?;
            for t in &world.teams {
                tx.execute("INSERT OR IGNORE INTO team_ids VALUES(?1)", [&t.id])?;
            }
            for b in &world.bots {
                tx.execute(
                    "INSERT OR IGNORE INTO bot_teams VALUES(?1,?2)",
                    params![b.id, b.team_id],
                )?;
            }
            tx.execute_batch("INSERT OR IGNORE INTO match_events SELECT m.id,json_extract(e.value,'$.seq'),e.value FROM matches m,json_each(m.events) e;
    INSERT OR IGNORE INTO match_history SELECT m.id,cast(h.key AS INTEGER),h.value FROM matches m,json_each(m.history) h;
    INSERT OR IGNORE INTO match_players SELECT m.id,json_extract(p.value,'$.id'),coalesce(b.team_id,''),m.created_ms FROM matches m,json_each(m.snapshot,'$.players') p LEFT JOIN bot_teams b ON b.bot_id=json_extract(p.value,'$.id') WHERE coalesce(json_extract(p.value,'$.house'),0)=0;
    UPDATE matches SET events='[]',history='[]';
    INSERT OR IGNORE INTO migrations VALUES(1);INSERT OR IGNORE INTO migrations VALUES(2);")?;
            tx.commit()?;
        }
        let indexed_bots = {
            let mut s = conn.prepare("SELECT bot_id FROM bot_teams")?;
            s.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let indexed_teams = {
            let mut s = conn.prepare("SELECT id FROM team_ids")?;
            s.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        Ok(Self {
            conn,
            _lease: lease,
            indexed_bots,
            indexed_teams,
            public_cache: None,
        })
    }
    pub fn reader(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_millis(250))?;
        conn.execute_batch("PRAGMA query_only=ON;")?;
        Ok(Self {
            conn,
            _lease: None,
            indexed_bots: HashSet::new(),
            indexed_teams: HashSet::new(),
            public_cache: None,
        })
    }
    pub fn world(&self) -> Result<World, String> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT data FROM world WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(sql_error)?;
        raw.map(|s| serde_json::from_str(&s).map_err(sql_error))
            .unwrap_or_else(|| Ok(World::default()))
    }
    /// Final snapshot, new events/checkpoints, ratings and tournament state commit together.
    pub fn save(&mut self, world: Option<&World>, records: &[Value]) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(sql_error)?;
        let mut bots = Vec::new();
        let mut teams = Vec::new();
        if let Some(w) = world {
            tx.execute(
                "UPDATE metadata_revision SET revision=revision+1 WHERE id=1",
                [],
            )
            .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO world VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
                [serde_json::to_string(w).map_err(sql_error)?],
            )
            .map_err(sql_error)?;
            for t in &w.teams {
                if !self.indexed_teams.contains(&t.id) {
                    tx.execute("INSERT OR IGNORE INTO team_ids VALUES(?1)", [&t.id])
                        .map_err(sql_error)?;
                    teams.push(t.id.clone());
                }
            }
            for b in &w.bots {
                if !self.indexed_bots.contains(&b.id) {
                    tx.execute(
                        "INSERT OR IGNORE INTO bot_teams VALUES(?1,?2)",
                        params![b.id, b.team_id],
                    )
                    .map_err(sql_error)?;
                    bots.push(b.id.clone());
                }
            }
        }
        for r in records {
            let mid = r["id"].as_str().ok_or("invalid_record_id")?;
            let exists = tx
                .query_row("SELECT 1 FROM matches WHERE id=?1", [mid], |r| {
                    r.get::<_, i64>(0)
                })
                .optional()
                .map_err(sql_error)?
                .is_some();
            tx.execute("INSERT INTO matches VALUES(?1,?2,?3,?4,?5,'[]','[]',?6,?7) ON CONFLICT(id) DO UPDATE SET status=excluded.status,updated_ms=excluded.updated_ms,snapshot=excluded.snapshot",params![mid,r["status"].as_str(),r["created_ms"].as_i64(),r["updated_ms"].as_i64(),r["snapshot"].to_string(),r["private_seed"].as_str(),r["tournament"].as_str()]).map_err(sql_error)?;
            if !exists {
                for p in r["snapshot"]["players"]
                    .as_array()
                    .ok_or("invalid_players")?
                    .iter()
                    .filter(|p| p["house"] != true)
                {
                    tx.execute("INSERT INTO match_players SELECT ?1,?2,coalesce((SELECT team_id FROM bot_teams WHERE bot_id=?2),''),?3",params![mid,p["id"].as_str(),r["created_ms"].as_i64()]).map_err(sql_error)?;
                }
            }
            for e in r["events"].as_array().ok_or("invalid_events")? {
                tx.execute("INSERT INTO match_events VALUES(?1,?2,?3) ON CONFLICT(match_id,seq) DO UPDATE SET data=excluded.data",params![mid,e["seq"].as_i64(),e.to_string()]).map_err(sql_error)?;
            }
            let start = r["history_start"].as_i64().unwrap_or(0);
            for (i, h) in r["history"]
                .as_array()
                .ok_or("invalid_history")?
                .iter()
                .enumerate()
            {
                tx.execute("INSERT INTO match_history VALUES(?1,?2,?3) ON CONFLICT(match_id,position) DO UPDATE SET data=excluded.data",params![mid,start+i as i64,h.to_string()]).map_err(sql_error)?;
            }
        }
        tx.commit().map_err(sql_error)?;
        self.indexed_bots.extend(bots);
        self.indexed_teams.extend(teams);
        Ok(())
    }
    pub fn record(&self, id: &str) -> Result<Value, String> {
        let mut r=self.conn.query_row("SELECT id,status,created_ms,updated_ms,snapshot,tournament FROM matches WHERE id=?1",[id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"created_ms":r.get::<_,i64>(2)?,"updated_ms":r.get::<_,i64>(3)?,"snapshot":parse(r,4)?,"tournament":r.get::<_,Option<String>>(5)?}))).optional().map_err(sql_error)?.ok_or("match_not_found")?;
        for (key, sql) in [
            (
                "events",
                "SELECT data FROM match_events WHERE match_id=?1 ORDER BY seq",
            ),
            (
                "history",
                "SELECT data FROM match_history WHERE match_id=?1 ORDER BY position",
            ),
        ] {
            let mut stmt = self.conn.prepare_cached(sql).map_err(sql_error)?;
            let data = stmt
                .query_map([id], |r| parse(r, 0))
                .map_err(sql_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(sql_error)?;
            r[key] = json!(data);
        }
        Ok(r)
    }
    pub fn recent(&self, limit: usize, offset: usize) -> Result<Vec<Value>, String> {
        let mut s=self.conn.prepare_cached("SELECT id,status,created_ms,snapshot,tournament FROM matches ORDER BY created_ms DESC,id LIMIT ?1 OFFSET ?2").map_err(sql_error)?;
        s.query_map(
            params![limit.min(100) as i64, offset.min(100000) as i64],
            summary,
        )
        .map_err(sql_error)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql_error)
    }
    pub fn history_for(&self, ids: &[String], offset: usize) -> Result<Vec<Value>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if ids.len() == 1 {
            let mut s=self.conn.prepare_cached("SELECT m.id,m.status,m.created_ms,m.snapshot,m.tournament FROM match_players p JOIN matches m ON m.id=p.match_id WHERE p.bot_id=?1 ORDER BY p.created_ms DESC,p.match_id LIMIT 50 OFFSET ?2").map_err(sql_error)?;
            return s
                .query_map(params![ids[0], offset.min(100000) as i64], summary)
                .map_err(sql_error)?
                .collect::<rusqlite::Result<_>>()
                .map_err(sql_error);
        }

        let mut s=self.conn.prepare_cached("SELECT id,status,created_ms,snapshot,tournament FROM matches WHERE id IN (SELECT match_id FROM match_players WHERE bot_id IN (SELECT value FROM json_each(?1))) ORDER BY created_ms DESC,id LIMIT 50 OFFSET ?2").map_err(sql_error)?;
        s.query_map(
            params![
                serde_json::to_string(ids).map_err(sql_error)?,
                offset.min(100000) as i64
            ],
            summary,
        )
        .map_err(sql_error)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql_error)
    }
    pub fn team_history(&self, id: &str, offset: usize) -> Result<Vec<Value>, String> {
        let mut s=self.conn.prepare_cached("SELECT m.id,m.status,m.created_ms,m.snapshot,m.tournament FROM match_players p JOIN matches m ON m.id=p.match_id WHERE p.team_id=?1 ORDER BY p.created_ms DESC,p.match_id LIMIT 50 OFFSET ?2").map_err(sql_error)?;
        s.query_map(params![id, offset.min(100000) as i64], summary)
            .map_err(sql_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql_error)
    }
    fn metadata(&mut self, kind: &str, offset: usize) -> Result<Value, String> {
        let revision: i64 = self
            .conn
            .query_row(
                "SELECT revision FROM metadata_revision WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if self
            .public_cache
            .as_ref()
            .is_none_or(|c| c.revision != revision)
        {
            let raw: Option<String> = self
                .conn
                .query_row("SELECT data FROM world WHERE id=1", [], |r| r.get(0))
                .optional()
                .map_err(sql_error)?;
            let world: PublicWorld = raw
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(sql_error)?
                .unwrap_or_default();
            self.public_cache = Some(PublicCache::new(revision, world));
        }
        let c = self.public_cache.as_ref().unwrap();
        match kind {
            "bots" | "leaderboard" => {
                let total = if kind == "bots" {
                    c.world.bots.len()
                } else {
                    c.ranked.len()
                };
                let bots: Vec<_> = if kind == "bots" {
                    (offset..total.min(offset + 100))
                        .map(|i| c.bot(i))
                        .collect()
                } else {
                    c.ranked
                        .iter()
                        .skip(offset)
                        .take(100)
                        .map(|&i| c.bot(i))
                        .collect()
                };
                Ok(
                    json!({"bots":bots,"total":total,"offset":offset,"next_offset":if offset+100<total{Some(offset+100)}else{None}}),
                )
            }
            "tournaments" => {
                let tournaments: Vec<_> = c
                    .world
                    .tournaments
                    .iter()
                    .rev()
                    .skip(offset)
                    .take(10)
                    .collect();
                let total = c.world.tournaments.len();
                let mut names = serde_json::Map::new();
                for t in &tournaments {
                    for e in &t.entrants {
                        names.insert(e.bot.clone(), json!(c.name(&e.bot)));
                    }
                }
                let standings:Vec<_>=tournaments.iter().map(|t|json!({"id":t.id,"entries":t.standings_with_buchholz().iter().map(|(e,buchholz)|json!({"bot":e.bot,"points":e.points,"buchholz":buchholz,"byes":e.byes})).collect::<Vec<_>>()})).collect();
                Ok(
                    json!({"tournaments":tournaments,"standings":standings,"names":names,"total":total,"offset":offset,"next_offset":if offset+10<total{Some(offset+10)}else{None}}),
                )
            }
            _ => Err("not_found".into()),
        }
    }
    pub fn archive(&mut self, kind: &str, id: &str, offset: usize) -> Result<Value, String> {
        if offset > crate::security::MAX_OFFSET {
            return Err("invalid_offset".into());
        }
        // One coherent WAL snapshot across header, event and history queries.
        let start = Instant::now();
        self.conn
            .progress_handler(
                1000,
                Some(move || start.elapsed() > Duration::from_millis(250)),
            )
            .map_err(sql_error)?;
        if let Err(e) = self.conn.execute_batch("BEGIN DEFERRED") {
            let _ = self.conn.progress_handler(0, None::<fn() -> bool>);
            return Err(e.to_string());
        }
        let result = (|| match kind {
            "bots" | "leaderboard" | "tournaments" => self.metadata(kind, offset),
            "match" | "replay" => self.record(id),
            "matches" => Ok(
                json!({"matches":self.recent(50,offset)?,"offset":offset,"next_offset":offset+50}),
            ),
            "bot_matches" => {
                if !self
                    .conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM bot_teams WHERE bot_id=?1)",
                        [id],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(sql_error)?
                {
                    return Err("bot_not_found".into());
                }
                Ok(
                    json!({"matches":self.history_for(&[id.into()],offset)?,"offset":offset,"next_offset":offset+50,"bot_ids":[id]}),
                )
            }
            "team_matches" => {
                if !self
                    .conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM team_ids WHERE id=?1)",
                        [id],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(sql_error)?
                {
                    return Err("team_not_found".into());
                }
                let ids = {
                    let mut q = self
                        .conn
                        .prepare_cached(
                            "SELECT bot_id FROM bot_teams WHERE team_id=?1 ORDER BY bot_id",
                        )
                        .map_err(sql_error)?;
                    q.query_map([id], |r| r.get::<_, String>(0))
                        .map_err(sql_error)?
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map_err(sql_error)?
                };
                Ok(
                    json!({"matches":self.team_history(id,offset)?,"offset":offset,"next_offset":offset+50,"bot_ids":ids}),
                )
            }
            _ => Err("not_found".into()),
        })();
        let clear = self
            .conn
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(sql_error);
        let end = self
            .conn
            .execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })
            .map_err(sql_error);
        end?;
        clear?;
        result.map_err(|e| {
            if e == "interrupted" || e.contains("locked") {
                "server_busy".into()
            } else {
                e
            }
        })
    }
    pub fn recover(&mut self, world: &mut World) -> Result<usize, String> {
        let rows: Vec<(String, String)> = {
            let mut s = self
                .conn
                .prepare("SELECT id,seed FROM matches WHERE status='live'")
                .map_err(sql_error)?;
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(sql_error)?
                .collect::<rusqlite::Result<_>>()
                .map_err(sql_error)?
        };
        let count = rows.len();
        let mut records = Vec::new();
        for (mid, seed) in rows {
            let mut r = self.record(&mid)?;
            r["status"] = json!("aborted");
            r["snapshot"]["finished"] = json!(true);
            r["snapshot"]["seed"] = json!(seed);
            r["snapshot"]["card"] = Value::Null;
            let seq = r["snapshot"]["seq"].as_u64().unwrap_or(0) + 1;
            r["snapshot"]["seq"] = json!(seq);
            let round = r["snapshot"]["round"].clone();
            r["events"].as_array_mut().ok_or("corrupt_events")?.push(json!({"seq":seq,"round":round,"kind":"aborted","data":{"seed":seed,"reason":"server_restart"}}));
            r["updated_ms"] = json!(crate::service::now_ms());
            r["private_seed"] = json!(seed);
            let terminal = r["snapshot"].clone();
            r["history"]
                .as_array_mut()
                .ok_or("corrupt_history")?
                .push(terminal);
            records.push(r);
            for t in &mut world.tournaments {
                for p in &mut t.pairings {
                    if p.match_id.as_deref() == Some(&mid) {
                        p.match_id = None;
                    }
                }
            }
        }
        self.save(Some(world), &records)?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_engine_can_own_a_database_inode() {
        let dir = std::env::temp_dir().join(format!("pokerdraft-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("db.sqlite");
        let path = path.to_str().unwrap();
        let a = Store::open(path).unwrap();
        assert!(Store::open(path).is_err());
        let ro = Store::reader(path).unwrap();
        assert!(ro.conn.execute("DELETE FROM matches", []).is_err());
        drop(ro);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let alias = dir.join("alias.sqlite");
            std::fs::hard_link(path, &alias).unwrap();
            assert!(Store::open(alias.to_str().unwrap()).is_err());
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(a);
        assert!(Store::open(path).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use pokerdraft_engine::{Game, Player, Rules};
    fn fixture() -> Value {
        let mut g = Game::new(
            Rules {
                seats: 2,
                ..Rules::default()
            },
            vec![
                Player::new("a".into(), "Alpha".into(), false),
                Player::new("b".into(), "Bravo".into(), false),
            ],
            [9; 32],
        )
        .unwrap();
        let mut history = vec![g.snapshot()];
        for _ in 0..52 {
            g.settle().unwrap();
            history.push(g.snapshot());
        }
        json!({"id":"fixture","status":"complete","created_ms":1,"updated_ms":2,"snapshot":g.snapshot(),"events":g.events,"history":history,"private_seed":hex::encode([9;32]),"tournament":null})
    }
    #[test]
    fn legacy_migration_preserves_replays_and_indexes() {
        let dir = std::env::temp_dir().join(format!("pokerdraft-migrate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("db.sqlite");
        let path = path.to_str().unwrap();
        let f = fixture();
        let c = Connection::open(path).unwrap();
        c.execute_batch("CREATE TABLE migrations(version INTEGER PRIMARY KEY);INSERT INTO migrations VALUES(1);CREATE TABLE world(id INTEGER PRIMARY KEY,data TEXT NOT NULL);CREATE TABLE matches(id TEXT PRIMARY KEY,status TEXT NOT NULL,created_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL,snapshot TEXT NOT NULL,events TEXT NOT NULL,history TEXT NOT NULL,seed TEXT NOT NULL,tournament TEXT);").unwrap();
        c.execute(
            "INSERT INTO matches VALUES('fixture','complete',1,2,?1,?2,?3,?4,NULL)",
            params![
                f["snapshot"].to_string(),
                f["events"].to_string(),
                f["history"].to_string(),
                f["private_seed"].as_str()
            ],
        )
        .unwrap();
        drop(c);
        let s = Store::open(path).unwrap();
        let r = s.record("fixture").unwrap();
        for key in ["snapshot", "events", "history"] {
            assert_eq!(r[key], f[key]);
        }
        assert!(pokerdraft_engine::replay::verify(r).is_ok());
        assert_eq!(s.history_for(&["a".into()], 0).unwrap().len(), 1);
        assert!(s.history_for(&["missing".into()], 0).unwrap().is_empty());
        assert_eq!(
            s.conn
                .query_row("SELECT events FROM matches WHERE id='fixture'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "[]"
        );
        let mut ro = Store::reader(path).unwrap();
        assert!(
            pokerdraft_engine::replay::verify(ro.archive("replay", "fixture", 0).unwrap()).is_ok()
        );
        drop(ro);
        drop(s);
        let s = Store::open(path).unwrap();
        assert_eq!(s.record("fixture").unwrap()["events"], f["events"]);
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn ledger_failure_rolls_back_world_metadata() {
        let mut s = Store::open(":memory:").unwrap();
        s.save(Some(&World::default()), &[]).unwrap();
        let changed = World {
            paused: true,
            ..World::default()
        };
        assert!(s.save(Some(&changed), &[json!({"id":"bad"})]).is_err());
        assert!(!s.world().unwrap().paused);
        assert!(s.recent(10, 0).unwrap().is_empty());
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use pokerdraft_engine::{Game, Player, Rules};
    #[test]
    fn archive_header_and_ledger_remain_one_snapshot_during_writes() {
        let dir = std::env::temp_dir().join(format!(
            "pokerdraft-read-consistency-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("db.sqlite");
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        let mut game = Game::new(
            Rules {
                seats: 2,
                ..Rules::default()
            },
            vec![
                Player::new("a".into(), "Alpha".into(), false),
                Player::new("b".into(), "Bravo".into(), false),
            ],
            [9; 32],
        )
        .unwrap();
        let mut history = vec![game.snapshot()];
        let save = |s: &mut Store, g: &Game, h: &Vec<pokerdraft_engine::Snapshot>| {
            s.save(None,&[json!({"id":"fixture","status":if g.snapshot().finished{"complete"}else{"live"},"created_ms":1,"updated_ms":2,"snapshot":g.snapshot(),"events":g.events,"history":h,"private_seed":hex::encode([9;32]),"tournament":null})]).unwrap()
        };
        save(&mut store, &game, &history);
        let readpath = path.clone();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let other = barrier.clone();
        let reader = std::thread::spawn(move || {
            let mut ro = Store::reader(readpath.to_str().unwrap()).unwrap();
            other.wait();
            for _ in 0..200 {
                let r = ro.archive("replay", "fixture", 0).unwrap();
                assert_eq!(
                    r["snapshot"]["seq"].as_u64().unwrap() as usize,
                    r["events"].as_array().unwrap().len()
                );
                assert_eq!(
                    r["history"].as_array().unwrap().last().unwrap(),
                    &r["snapshot"]
                );
            }
        });
        barrier.wait();
        for _ in 0..52 {
            game.settle().unwrap();
            history.push(game.snapshot());
            save(&mut store, &game, &history);
        }
        reader.join().unwrap();
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;
    #[test]
    fn future_schema_refused_before_creating_legacy_tables() {
        let dir = std::env::temp_dir().join(format!("pokerdraft-version-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("db.sqlite");
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE migrations(version INTEGER PRIMARY KEY);INSERT INTO migrations VALUES(99);").unwrap();
        drop(c);
        assert!(Store::open(path.to_str().unwrap()).is_err());
        let c = Connection::open(&path).unwrap();
        let exists: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='world')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists);
        drop(c);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod public_cache_tests {
    use super::*;
    #[test]
    fn metadata_is_paged_redacted_and_invalidated_atomically() {
        use crate::service::{Bot, Team};
        let mut s = Store::open(":memory:").unwrap();
        let mut w = World::default();
        w.teams.push(Team {
            id: "team".into(),
            name: "Alpha".into(),
            owner_hash: "PRIVATE_OWNER_HASH".into(),
            disabled: false,
            created_ms: 1,
        });
        for i in 0..105 {
            w.bots.push(Bot {
                id: format!("bot-{i:03}"),
                team_id: "team".into(),
                name: format!("v{i}"),
                token_hash: format!("PRIVATE_BOT_HASH_{i}"),
                rating: 1500. + i as f64,
                games: 1,
                disabled: false,
                created_ms: 1,
            });
        }
        s.save(Some(&w), &[]).unwrap();
        let first = s.archive("bots", "", 0).unwrap();
        assert_eq!(first["bots"].as_array().unwrap().len(), 100);
        assert_eq!(first["total"], 105);
        assert!(!first.to_string().contains("PRIVATE_"));
        let last = s.archive("bots", "", 100).unwrap();
        assert_eq!(last["bots"].as_array().unwrap().len(), 5);
        assert!(last["next_offset"].is_null());
        assert_eq!(
            s.archive("leaderboard", "", 0).unwrap()["bots"][0]["id"],
            "bot-104"
        );
        w.teams[0].disabled = true;
        s.save(Some(&w), &[]).unwrap();
        assert_eq!(s.archive("leaderboard", "", 0).unwrap()["total"], 0);
        assert_eq!(
            s.archive("bots", "", 0).unwrap()["bots"][0]["disabled"],
            true
        );
    }
}
