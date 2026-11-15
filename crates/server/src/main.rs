mod security;
mod service;
mod store;
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum::{
    extract::Request,
    middleware::{self, Next},
};
use futures_util::{SinkExt, StreamExt};
use security::{Quota, Security, public_error, validate_request};
use serde::Deserialize;
use serde_json::{Value, json};
use service::Handle;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
#[derive(Clone)]
struct App {
    handle: Handle,
    security: Arc<Security>,
    connections: Arc<tokio::sync::Semaphore>,
    http: Arc<tokio::sync::Semaphore>,
}
async fn admission(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if !a.security.host(request.headers()) {
        return (StatusCode::FORBIDDEN, "host_rejected").into_response();
    }
    if !a.security.origin(request.headers()) {
        return (StatusCode::FORBIDDEN, "origin_rejected").into_response();
    }
    let api = request.uri().path().starts_with("/api/")
        || matches!(request.uri().path(), "/healthz" | "/metrics");
    let quota = if request.method() == axum::http::Method::POST {
        Quota::Command
    } else {
        Quota::Read
    };
    if !a.security.admit(peer.ip(), quota) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"ok":false,"error":"rate_limited"})),
        )
            .into_response();
    }
    let Ok(_permit) = a.http.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let mut response = match tokio::time::timeout(Duration::from_secs(5), next.run(request)).await {
        Ok(r) => r,
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    if api {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        );
    }
    response
}
fn response(r: Result<Value, String>) -> Response {
    match r {
        Ok(v) => (StatusCode::OK, Json(json!({"ok":true,"result":v}))).into_response(),
        Err(e) => {
            let e = public_error(&e);
            let status = if e.contains("auth_required") {
                StatusCode::UNAUTHORIZED
            } else if e.ends_with("not_found") {
                StatusCode::NOT_FOUND
            } else if e == "server_busy" || e == "service_unavailable" || e == "internal_error" {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(json!({"ok":false,"error":e}))).into_response()
        }
    }
}
#[derive(Deserialize)]
struct Paging {
    #[serde(default)]
    offset: usize,
}
async fn read(State(a): State<App>, Path(kind): Path<String>, Query(p): Query<Paging>) -> Response {
    if p.offset > security::MAX_OFFSET {
        return response(Err("invalid_offset".into()));
    }
    response(a.handle.read(&kind, "", p.offset).await)
}
async fn read_match(State(a): State<App>, Path(id): Path<String>) -> Response {
    response(a.handle.read("match", &id, 0).await)
}
async fn bot_history(
    State(a): State<App>,
    Path(id): Path<String>,
    Query(p): Query<Paging>,
) -> Response {
    if p.offset > security::MAX_OFFSET {
        return response(Err("invalid_offset".into()));
    }
    response(a.handle.read("bot_matches", &id, p.offset).await)
}
async fn team_history(
    State(a): State<App>,
    Path(id): Path<String>,
    Query(p): Query<Paging>,
) -> Response {
    if p.offset > security::MAX_OFFSET {
        return response(Err("invalid_offset".into()));
    }
    response(a.handle.read("team_matches", &id, p.offset).await)
}
async fn replay(State(a): State<App>, Path(id): Path<String>) -> Response {
    response(a.handle.read("replay", &id, 0).await)
}
async fn command(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if validate_request(&body).is_err() {
        return response(Err("invalid_request".into()));
    }
    if body["op"] == "create_team" && !a.security.admit(peer.ip(), Quota::Register) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"ok":false,"error":"rate_limited"})),
        )
            .into_response();
    }
    if !["bid", "cash", "join", "leave", "auth"].contains(&body["op"].as_str().unwrap_or(""))
        && !a.security.admit(peer.ip(), Quota::Control)
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"ok":false,"error":"rate_limited"})),
        )
            .into_response();
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    response(a.handle.command(token, body).await)
}
async fn ws(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !a.security.origin(&headers) || !a.security.admit(peer.ip(), Quota::Connect) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = a.connections.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(peer_permit) = a.security.sockets(peer.ip()).try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    upgrade
        .max_message_size(8192)
        .max_frame_size(8192)
        .on_upgrade(move |socket| async move {
            let _permit = (permit, peer_permit);
            session(socket, a, peer.ip()).await
        })
        .into_response()
}
async fn session(socket: WebSocket, a: App, peer: IpAddr) {
    let mut feed = a.handle.feed.subscribe();
    let (mut sink, mut source) = socket.split();
    let mut token = String::new();
    let mut bot = String::new();
    let mut watch = String::new();
    let Ok(initial) = a.handle.read("directory", "", 0).await else {
        return;
    };
    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(3),
            sink.send(Message::Text(initial.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    let mut window = Instant::now();
    let mut count = 0u32;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
    let mut last_seen = Instant::now();
    loop {
        let outgoing = tokio::select! {
            input=source.next()=>{
                let Some(Ok(message))=input else{break;};
                if window.elapsed()>=Duration::from_secs(1){window=Instant::now();count=0;}
                count+=1;
                if count>30||!a.security.admit(peer,Quota::Command){break;}
                last_seen=Instant::now();
                match message {
                    Message::Text(text)=>{
                        let b:Value=match serde_json::from_str(text.as_str()){Ok(v)=>v,Err(_)=>break};
                        let (op,rid)=match validate_request(&b){Ok(v)=>v,Err(_)=>break};
                        let r=match op {
                            "ping"=>Ok(json!({"server_time_ms":service::now_ms()})),
                            "watch"=>{if let Some(id)=b["match_id"].as_str(){match a.handle.read("match",id,0).await{Ok(v)=>{watch=id.into();Ok(json!({"match":v}))},Err(e)=>Err(e)}}else{Err("missing_match_id".into())}},
                            "auth"=>{
                                token.clear();bot.clear();
                                if let Some(t)=b["token"].as_str(){match a.handle.command(t.into(),json!({"op":"auth","id":rid})).await {Ok(v)=>{token=t.into();bot=v["bot_id"].as_str().unwrap_or("").into();Ok(v)},Err(e)=>Err(e)}}else{Err("missing_token".into())}
                            },
                            "bid"|"cash"|"join"|"leave"=>a.handle.command(token.clone(),b.clone()).await,
                            _=>Err("transport_not_allowed".into()),
                        };
                        Some(Message::Text(match r{Ok(v)=>json!({"type":"ack","id":rid,"ok":true,"result":v}),Err(e)=>json!({"type":"ack","id":rid,"ok":false,"error":public_error(&e)})}.to_string().into()))
                    },
                    Message::Ping(p)=>Some(Message::Pong(p)),Message::Close(_)=>break,_=>None
                }
            },
            event=feed.recv()=>match event {
                Ok(f)=>if f.topic=="directory"||f.topic==format!("match:{watch}")||(!bot.is_empty()&&f.bots.contains(&bot)){Some(Message::Text(f.value.to_string().into()))}else{None},
                Err(_)=>break,
            },
            _=heartbeat.tick()=>{if last_seen.elapsed()>Duration::from_secs(60){break;}Some(Message::Text(json!({"type":"heartbeat","server_time_ms":service::now_ms()}).to_string().into()))}
        };
        if let Some(v) = outgoing
            && !matches!(
                tokio::time::timeout(Duration::from_secs(3), sink.send(v)).await,
                Ok(Ok(()))
            )
        {
            break;
        }
    }
}
async fn health(State(a): State<App>) -> Response {
    response(a.handle.read("health", "", 0).await)
}
async fn metrics(State(a): State<App>) -> Response {
    match a.handle.read("health", "", 0).await {
        Ok(v) => {
            let s = format!(
                "# TYPE pokerdraft_commands_total counter\npokerdraft_commands_total {}\n# TYPE pokerdraft_rejected_total counter\npokerdraft_rejected_total {}\n# TYPE pokerdraft_active_matches gauge\npokerdraft_active_matches {}\n# TYPE pokerdraft_max_tick_lag_ms gauge\npokerdraft_max_tick_lag_ms {}\n",
                v["commands"], v["rejected"], v["active_matches"], v["max_tick_lag_ms"]
            );
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; version=0.0.4",
                )],
                s,
            )
                .into_response()
        }
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pokerdraft_server=info,tower_http=info".into()),
        )
        .init();
    let db = std::env::var("POKERDRAFT_DB").unwrap_or_else(|_| "data/pokerdraft.db".into());
    if let Some(parent) = std::path::Path::new(&db).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let rules = pokerdraft_engine::Rules {
        seats: std::env::var("POKERDRAFT_SEATS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(6),
        auction_ms: std::env::var("POKERDRAFT_AUCTION_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000),
        pricing: if std::env::var("POKERDRAFT_PRICING").as_deref() == Ok("first_price") {
            pokerdraft_engine::Pricing::FirstPrice
        } else {
            pokerdraft_engine::Pricing::SecondPrice
        },
    };
    let admin = std::env::var("POKERDRAFT_ADMIN_TOKEN").ok();
    if admin.as_ref().is_some_and(|s| s.len() < 32) {
        return Err("POKERDRAFT_ADMIN_TOKEN must contain at least 32 characters".into());
    }
    let handle = service::Service::spawn(
        &db,
        admin,
        rules,
        std::env::var("POKERDRAFT_DEMO").as_deref() != Ok("false"),
    )?;
    let security = Security::new(
        &std::env::var("POKERDRAFT_ALLOWED_HOSTS")
            .unwrap_or_else(|_| "localhost,127.0.0.1,[::1]".into()),
        &std::env::var("POKERDRAFT_ALLOWED_ORIGINS").unwrap_or_default(),
    )?;
    let app = App {
        handle: handle.clone(),
        security: Arc::new(security),
        connections: Arc::new(tokio::sync::Semaphore::new(2048)),
        http: Arc::new(tokio::sync::Semaphore::new(256)),
    };
    let web = std::env::var("POKERDRAFT_WEB").unwrap_or_else(|_| "web/dist".into());
    let router=Router::new().route("/api/command",post(command)).route("/api/bots/{id}/matches",get(bot_history)).route("/api/teams/{id}/matches",get(team_history)).route("/api/matches/{id}/replay",get(replay)).route("/api/matches/{id}",get(read_match)).route("/api/{kind}",get(read)).route("/healthz",get(health)).route("/metrics",get(metrics)).route("/ws",get(ws))
  .fallback_service(ServeDir::new(&web).not_found_service(ServeFile::new(format!("{web}/index.html"))))
  .layer(axum::extract::DefaultBodyLimit::max(8192))
  .layer(SetResponseHeaderLayer::if_not_present(axum::http::header::X_CONTENT_TYPE_OPTIONS,HeaderValue::from_static("nosniff")))
  .layer(SetResponseHeaderLayer::if_not_present(axum::http::header::REFERRER_POLICY,HeaderValue::from_static("no-referrer")))
  .layer(SetResponseHeaderLayer::if_not_present(axum::http::header::CONTENT_SECURITY_POLICY,HeaderValue::from_static("default-src 'self'; connect-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'self'")))
  .layer(SetResponseHeaderLayer::if_not_present(axum::http::header::X_FRAME_OPTIONS,HeaderValue::from_static("DENY")))
  .layer(SetResponseHeaderLayer::if_not_present(axum::http::header::HeaderName::from_static("permissions-policy"),HeaderValue::from_static("camera=(), microphone=(), geolocation=()")))
  .layer(middleware::from_fn_with_state(app.clone(),admission))
  .layer(TraceLayer::new_for_http().make_span_with(|r:&Request|tracing::info_span!("http",method=%r.method(),path=%r.uri().path())))
  .with_state(app);
    let bind = std::env::var("POKERDRAFT_BIND").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind,"Poker Draft listening");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        #[cfg(unix)]
        {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
        handle.shutdown().await;
    })
    .await?;
    Ok(())
}
