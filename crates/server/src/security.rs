//! Admission controls shared by HTTP and WebSocket transports.
use axum::http::{HeaderMap, uri::Authority};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub const MAX_OFFSET: usize = 100_000;
#[derive(Clone, Copy)]
pub enum Quota {
    Read,
    Command,
    Register,
    Connect,
    Control,
}
struct Bucket {
    start: Instant,
    counts: [u32; 5],
    sockets: Arc<Semaphore>,
}
pub struct Security {
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
    hosts: HashSet<String>,
    origins: HashSet<String>,
}
impl Security {
    pub fn new(hosts: &str, origins: &str) -> Result<Self, String> {
        let hosts = hosts
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| authority_host(s).ok_or_else(|| "invalid_allowed_host".to_owned()))
            .collect::<Result<HashSet<_>, _>>()?;
        if hosts.is_empty() {
            return Err("allowed_hosts_required".into());
        }
        let mut allowed = HashSet::new();
        for origin in origins.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let uri: axum::http::Uri = origin.parse().map_err(|_| "invalid_allowed_origin")?;
            if !matches!(uri.scheme_str(), Some("http" | "https"))
                || uri.authority().is_none()
                || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
                || origin.contains('@')
            {
                return Err("invalid_allowed_origin".into());
            }
            allowed.insert(origin.to_owned());
        }
        Ok(Self {
            buckets: Mutex::new(HashMap::new()),
            hosts,
            origins: allowed,
        })
    }
    pub fn host(&self, h: &HeaderMap) -> bool {
        h.get("host")
            .and_then(|h| h.to_str().ok())
            .and_then(authority_host)
            .is_some_and(|host| self.hosts.contains(&host))
    }
    pub fn origin(&self, h: &HeaderMap) -> bool {
        let Some(origin) = h.get("origin") else {
            return true;
        };
        let Ok(origin) = origin.to_str() else {
            return false;
        };
        if self.origins.contains(origin) {
            return true;
        }
        let Some(host) = h.get("host").and_then(|s| s.to_str().ok()) else {
            return false;
        };
        origin == format!("http://{host}") || origin == format!("https://{host}")
    }
    pub fn admit(&self, ip: IpAddr, quota: Quota) -> bool {
        let mut map = self.buckets.lock().unwrap();
        if map.len() >= 16384 && !map.contains_key(&ip) {
            map.retain(|_, b| {
                b.start.elapsed() < Duration::from_secs(60) || b.sockets.available_permits() < 32
            });
            if map.len() >= 16384 {
                return false;
            }
        }
        let b = map.entry(ip).or_insert_with(|| Bucket {
            start: Instant::now(),
            counts: [0; 5],
            sockets: Arc::new(Semaphore::new(32)),
        });
        if b.start.elapsed() >= Duration::from_secs(60) {
            b.start = Instant::now();
            b.counts = [0; 5];
        }
        let i = quota as usize;
        let max = [3000, 6000, 5, 30, 60][i];
        if b.counts[i] >= max {
            return false;
        }
        b.counts[i] += 1;
        true
    }
    pub fn sockets(&self, ip: IpAddr) -> Arc<Semaphore> {
        self.buckets
            .lock()
            .unwrap()
            .get(&ip)
            .expect("admit connect before socket lease")
            .sockets
            .clone()
    }
}
fn authority_host(s: &str) -> Option<String> {
    if s.contains('@') {
        return None;
    }
    let a: Authority = s.parse().ok()?;
    Some(a.host().trim_matches(['[', ']']).to_ascii_lowercase())
}

pub fn validate_request(b: &Value) -> Result<(&str, &str), String> {
    let o = b.as_object().ok_or("invalid_request")?;
    let op = o.get("op").and_then(Value::as_str).ok_or("missing_op")?;
    let id = o
        .get("id")
        .and_then(Value::as_str)
        .ok_or("missing_request_id")?;
    if id.is_empty() || id.len() > 100 || !id.bytes().all(|c| c.is_ascii_graphic()) {
        return Err("invalid_request_id".into());
    }
    let fields: &[&str] = match op {
        "auth" => &["token"],
        "watch" | "admin_abort_match" => &["match_id"],
        "ping" | "join" | "leave" | "account" | "rotate_owner" => &[],
        "create_team" | "create_bot" => &["name"],
        "rotate_bot" => &["bot_id"],
        "bid" => &["match_id", "round", "amount"],
        "cash" => &["match_id", "round", "cards"],
        "register_tournament" | "unregister_tournament" => &["bot_id", "tournament_id"],
        "admin_create_tournament" => &["name", "swiss_rounds", "cut"],
        "admin_start_tournament" | "admin_cancel_tournament" => &["tournament_id"],
        "admin_pause" => &["paused"],
        "admin_disable_team" => &["team_id", "disabled"],
        _ => return Err("unknown_operation".into()),
    };
    if o.keys()
        .any(|k| k != "id" && k != "op" && !fields.contains(&k.as_str()))
    {
        return Err("unknown_field".into());
    }
    for key in ["token", "match_id", "bot_id", "team_id", "tournament_id"] {
        if let Some(v) = o.get(key) {
            let s = v.as_str().ok_or("invalid_field")?;
            if s.len() > 128 {
                return Err("invalid_field".into());
            }
        }
    }
    Ok((op, id))
}
pub fn public_error(e: &str) -> &str {
    if e.len() <= 80 && !e.is_empty() && e.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        e
    } else {
        "internal_error"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reject_rebinding_and_bad_origins() {
        let s = Security::new("localhost,127.0.0.1,[::1],arena.example", "").unwrap();
        let mut h = HeaderMap::new();
        h.insert("host", "attacker.example:3000".parse().unwrap());
        h.insert("origin", "http://attacker.example:3000".parse().unwrap());
        assert!(!s.host(&h));
        h.insert("host", "localhost:3000".parse().unwrap());
        assert!(s.host(&h));
        assert!(!s.origin(&h));
        h.insert("origin", "null".parse().unwrap());
        assert!(!s.origin(&h));
        h.insert("origin", "http://localhost:3000".parse().unwrap());
        assert!(s.origin(&h));
    }
    #[test]
    fn shared_registration_and_connection_limits() {
        let s = Security::new("localhost", "").unwrap();
        let ip = "127.0.0.1".parse().unwrap();
        for _ in 0..5 {
            assert!(s.admit(ip, Quota::Register));
        }
        assert!(!s.admit(ip, Quota::Register));
        assert!(s.admit(ip, Quota::Connect));
        let sem = s.sockets(ip);
        let permits: Vec<_> = (0..32)
            .map(|_| sem.clone().try_acquire_owned().unwrap())
            .collect();
        assert!(sem.clone().try_acquire_owned().is_err());
        drop(permits);
        assert_eq!(sem.available_permits(), 32);
    }
    #[test]
    fn strict_ids_shapes_and_error_redaction() {
        for v in [
            json!([]),
            json!({"op":"ping","id":{}}),
            json!({"op":"ping","id":"x","token":"secret"}),
            json!({"op":"bid","id":"\n"}),
        ] {
            assert!(validate_request(&v).is_err());
        }
        assert!(
            validate_request(&json!({"op":"bid","id":"ok","round":0,"match_id":"x","amount":1}))
                .is_ok()
        );
        assert_eq!(
            public_error("storage_failure: /private/db"),
            "internal_error"
        );
    }
}
