use crate::service::World;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

pub struct Store {
    pub conn: Connection,
}
impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
   CREATE TABLE IF NOT EXISTS migrations(version INTEGER PRIMARY KEY);
   CREATE TABLE IF NOT EXISTS world(id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS matches(id TEXT PRIMARY KEY,status TEXT NOT NULL,created_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL,snapshot TEXT NOT NULL,events TEXT NOT NULL,history TEXT NOT NULL,seed TEXT NOT NULL,tournament TEXT);
   CREATE INDEX IF NOT EXISTS match_recent ON matches(created_ms DESC);
   INSERT OR IGNORE INTO migrations VALUES(1);")?;
        Ok(Self { conn })
    }
    pub fn world(&self) -> Result<World, String> {
        let s: Option<String> = self
            .conn
            .query_row("SELECT data FROM world WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        s.map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .unwrap_or_else(|| Ok(World::default()))
    }
    /// Result snapshot, history, world ratings and tournament state commit together.
    pub fn save(&mut self, world: Option<&World>, records: &[Value]) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if let Some(world) = world {
            tx.execute(
                "INSERT INTO world VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
                [serde_json::to_string(world).map_err(|e| e.to_string())?],
            )
            .map_err(|e| e.to_string())?;
        }
        for r in records {
            tx.execute("INSERT INTO matches VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET status=excluded.status,updated_ms=excluded.updated_ms,snapshot=excluded.snapshot,events=excluded.events,history=excluded.history",params![r["id"].as_str(),r["status"].as_str(),r["created_ms"].as_i64(),r["updated_ms"].as_i64(),r["snapshot"].to_string(),r["events"].to_string(),r["history"].to_string(),r["private_seed"].as_str(),r["tournament"].as_str()]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn record(&self, id: &str) -> Result<Value, String> {
        self.conn.query_row("SELECT id,status,created_ms,updated_ms,snapshot,events,history,tournament FROM matches WHERE id=?1",[id],|r| {
   let parse=|i|->rusqlite::Result<Value>{let s:String=r.get(i)?;Ok(serde_json::from_str(&s).unwrap_or(Value::Null))};
   Ok(json!({"id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"created_ms":r.get::<_,i64>(2)?,"updated_ms":r.get::<_,i64>(3)?,"snapshot":parse(4)?,"events":parse(5)?,"history":parse(6)?,"tournament":r.get::<_,Option<String>>(7)?}))
  }).optional().map_err(|e|e.to_string())?.ok_or_else(||"match_not_found".into())
    }
    pub fn recent(&self, limit: usize, offset: usize) -> Result<Vec<Value>, String> {
        let mut stmt=self.conn.prepare("SELECT id,status,created_ms,snapshot,tournament FROM matches ORDER BY created_ms DESC,id LIMIT ?1 OFFSET ?2").map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![limit.min(100) as i64,offset.min(i64::MAX as usize) as i64],|r| {
   let snap:String=r.get(3)?;let snap:Value=serde_json::from_str(&snap).unwrap_or(Value::Null);
   Ok(json!({"id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"created_ms":r.get::<_,i64>(2)?,"players":snap["players"],"round":snap["round"],"tournament":r.get::<_,Option<String>>(4)?}))
  }).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    pub fn history_for(&self, ids: &[String], offset: usize) -> Result<Vec<Value>, String> {
        let ids = serde_json::to_string(ids).map_err(|e| e.to_string())?;
        let mut stmt = self.conn.prepare("SELECT id,status,created_ms,snapshot,tournament FROM matches WHERE EXISTS (SELECT 1 FROM json_each(matches.snapshot,'$.players') p WHERE json_extract(p.value,'$.id') IN (SELECT value FROM json_each(?1))) ORDER BY created_ms DESC,id LIMIT 50 OFFSET ?2").map_err(|e|e.to_string())?;
        let rows = stmt.query_map(params![ids,offset.min(i64::MAX as usize) as i64],|r| {
            let snap:String=r.get(3)?;
            let snap:Value=serde_json::from_str(&snap).unwrap_or(Value::Null);
            Ok(json!({"id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"created_ms":r.get::<_,i64>(2)?,"players":snap["players"],"round":snap["round"],"tournament":r.get::<_,Option<String>>(4)?}))
        }).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    pub fn recover(&mut self, world: &mut World) -> Result<usize, String> {
        let rows: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id,seed FROM matches WHERE status='live'")
                .map_err(|e| e.to_string())?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        };
        let count = rows.len();
        let mut records = Vec::new();
        for (id, seed) in rows {
            let mut r = self.record(&id)?;
            r["status"] = json!("aborted");
            r["snapshot"]["finished"] = json!(true);
            r["snapshot"]["seed"] = json!(seed);
            r["snapshot"]["card"] = Value::Null;
            let round = r["snapshot"]["round"].clone();
            let seq = r["snapshot"]["seq"].as_u64().unwrap_or(0) + 1;
            r["snapshot"]["seq"] = json!(seq);
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
                    if p.match_id.as_deref() == Some(&id) {
                        p.match_id = None;
                    }
                }
            }
        }
        self.save(Some(world), &records)?;
        Ok(count)
    }
}
