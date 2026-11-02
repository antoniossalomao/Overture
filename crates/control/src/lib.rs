//! Local LAN HTTP + WebSocket remote control server (`axum`) with fixed-token auth and mobile web UI.

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use overture_core::{
    BibleVerse, CoreCommand, CoreEvent, LivePresentationItem, OutputState, Song, StateEngine,
};
use overture_storage::Storage;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower_http::cors::CorsLayer;
use tracing::info;

const MOBILE_HTML: &str = include_str!("mobile_ui.html");

#[derive(Clone)]
pub struct ControlState {
    pub engine: StateEngine,
    pub storage: Storage,
    pub expected_token: Arc<RwLock<String>>,
}

impl ControlState {
    pub fn new(engine: StateEngine, storage: Storage, token: impl Into<String>) -> Self {
        Self {
            engine,
            storage,
            expected_token: Arc::new(RwLock::new(token.into())),
        }
    }

    pub fn set_token(&self, new_token: impl Into<String>) {
        *self.expected_token.write().expect("token lock") = new_token.into();
    }

    fn verify_token(&self, candidate: Option<&str>) -> bool {
        let expected = self.expected_token.read().expect("token lock");
        if expected.trim().is_empty() {
            return false;
        }
        candidate.map(|c| c.trim() == expected.as_str()).unwrap_or(false)
    }
}

pub struct ControlServerHandle {
    pub bound_addr: SocketAddr,
    pub shared: ControlState,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl ControlServerHandle {
    pub fn lan_urls(&self) -> Vec<String> {
        let port = self.bound_addr.port();
        let mut urls = Vec::new();
        for ip in detect_local_lan_ipv4s() {
            urls.push(format!("http://{ip}:{port}"));
        }
        if urls.is_empty() {
            urls.push(format!("http://127.0.0.1:{port}"));
        }
        urls
    }

    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ControlServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

pub async fn start_control_server(
    bind_addr: SocketAddr,
    shared: ControlState,
) -> Result<ControlServerHandle> {
    let app = build_router(shared.clone());
    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("Failed to bind control server on {bind_addr}"))?;
    let actual_addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    info!("Overture remote control listening on http://{}", actual_addr);

    tokio::spawn(async move {
        let svc = app.into_make_service_with_connect_info::<SocketAddr>();
        let server = axum::serve(listener, svc).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        let _ = server.await;
    });

    Ok(ControlServerHandle {
        bound_addr: actual_addr,
        shared,
        shutdown_tx: Some(shutdown_tx),
    })
}

pub fn build_router(state: ControlState) -> Router {
    Router::new()
        .route("/", get(serve_mobile_ui))
        .route("/api/state", get(get_state))
        .route("/api/command", post(post_command))
        .route("/api/next", post(post_next))
        .route("/api/prev", post(post_prev))
        .route("/api/goto/{index}", post(post_goto))
        .route("/api/songs", get(search_songs_api))
        .route("/api/songs/{id}/present", post(present_song_api))
        .route(
            "/api/service/{service_id}/item/{item_id}/present",
            post(present_service_item_api),
        )
        .route("/api/bible", get(search_bible_api))
        .route("/api/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Checks that peer address belongs to loopback or RFC1918/link-local LAN.
pub fn is_lan_or_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unicast_link_local() || v6.is_unique_local() {
                return true;
            }
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_loopback() || mapped.is_private() || mapped.is_link_local();
            }
            false
        }
    }
}

fn check_lan_and_token(
    peer: SocketAddr,
    headers: &HeaderMap,
    query_token: Option<&str>,
    state: &ControlState,
) -> std::result::Result<(), StatusCode> {
    if !is_lan_or_loopback_ip(peer.ip()) {
        return Err(StatusCode::FORBIDDEN);
    }

    let header_tok = headers
        .get("x-overture-token")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer ").or_else(|| s.strip_prefix("bearer ")))
        })
        .or(query_token);

    if state.verify_token(header_tok) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn serve_mobile_ui(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> std::result::Result<Html<&'static str>, StatusCode> {
    if !is_lan_or_loopback_ip(peer.ip()) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Html(MOBILE_HTML))
}

async fn get_state(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<ControlState>,
) -> std::result::Result<Json<OutputState>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    Ok(Json(state.engine.snapshot()))
}

async fn post_command(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<ControlState>,
    Json(cmd): Json<CoreCommand>,
) -> std::result::Result<Json<OutputState>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    Ok(Json(state.engine.dispatch(cmd)))
}

async fn post_next(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<ControlState>,
) -> std::result::Result<Json<OutputState>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    Ok(Json(state.engine.dispatch(CoreCommand::NextSlide)))
}

async fn post_prev(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<ControlState>,
) -> std::result::Result<Json<OutputState>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    Ok(Json(state.engine.dispatch(CoreCommand::PrevSlide)))
}

async fn post_goto(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(index): Path<usize>,
    State(state): State<ControlState>,
) -> std::result::Result<Json<OutputState>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    Ok(Json(
        state.engine.dispatch(CoreCommand::GotoSlide { index }),
    ))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default = "default_version_id")]
    version_id: i64,
}

fn default_version_id() -> i64 {
    1
}

async fn search_songs_api(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
    State(state): State<ControlState>,
) -> std::result::Result<Json<Vec<Song>>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    let songs = state
        .storage
        .search_songs(&query.q, false, None, 40)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(songs))
}

async fn present_song_api(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    State(state): State<ControlState>,
) -> std::result::Result<Json<OutputState>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    let song = state
        .storage
        .get_song(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let theme = state.engine.snapshot().main_theme;
    let item = LivePresentationItem::from_song(&song, None, theme);
    let _ = state.storage.record_execution(None, Some(song.id)).await;
    Ok(Json(state.engine.dispatch(CoreCommand::PresentItem {
        item: Box::new(item),
        start_slide: 0,
    })))
}

async fn present_service_item_api(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((service_id, item_id)): Path<(i64, i64)>,
    State(state): State<ControlState>,
) -> std::result::Result<Json<OutputState>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    let svc = state
        .storage
        .get_service(service_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let svc_item = svc
        .items
        .into_iter()
        .find(|i| i.id == item_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let fallback = state.engine.snapshot().main_theme;
    let live = state
        .storage
        .resolve_service_item(&svc_item, &fallback)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = state
        .storage
        .record_execution(Some(svc_item.id), live.song_id)
        .await;
    Ok(Json(state.engine.dispatch(CoreCommand::PresentItem {
        item: Box::new(live),
        start_slide: 0,
    })))
}

async fn search_bible_api(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
    State(state): State<ControlState>,
) -> std::result::Result<Json<Vec<BibleVerse>>, StatusCode> {
    check_lan_and_token(peer, &headers, None, &state)?;
    let verses = state
        .storage
        .search_bible(query.version_id, &query.q, 40)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(verses))
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<WsQuery>,
    State(state): State<ControlState>,
) -> std::result::Result<Response, StatusCode> {
    check_lan_and_token(peer, &headers, q.token.as_deref(), &state)?;
    Ok(ws
        .on_upgrade(move |socket| handle_ws_connection(socket, state))
        .into_response())
}

async fn handle_ws_connection(socket: WebSocket, state: ControlState) {
    let (mut sender, mut receiver) = socket.split();
    let mut events_rx = state.engine.subscribe_events();

    // Immediately push initial OutputUpdated snapshot upon WebSocket connection
    let initial = CoreEvent::OutputUpdated {
        state: Box::new(state.engine.snapshot()),
    };
    if let Ok(json) = serde_json::to_string(&initial)
        && sender.send(Message::Text(json.into())).await.is_err()
    {
        return;
    }

    let send_task = tokio::spawn(async move {
        while let Ok(ev) = events_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&ev)
                && sender.send(Message::Text(json.into())).await.is_err()
            {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(txt) = msg
            && let Ok(cmd) = serde_json::from_str::<CoreCommand>(&txt)
        {
            state.engine.dispatch(cmd);
        }
    }

    send_task.abort();
}

/// Detects primary local LAN IPv4 addresses via UDP socket routing table lookup (no packets sent).
pub fn detect_local_lan_ipv4s() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0")
        && sock.connect("192.168.1.1:80").is_ok()
        && let Ok(local) = sock.local_addr()
        && !local.ip().is_unspecified()
    {
        out.push(local.ip().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use overture_core::{SongSlide};
    use overture_storage::SongInsertOutcome;
    use std::time::Duration;

    #[tokio::test]
    async fn phase6_mobile_http_and_websocket_remote_controls_next_slide() -> Result<()> {
        let storage = Storage::connect_memory().await?;
        let mut song = Song::new_draft("Rocha Eterna", "Augustus Toplady", "public_domain");
        song.slides = vec![
            SongSlide {
                id: 0,
                song_id: 0,
                order: 0,
                text: "Rocha eterna, foi na cruz\nQue morreste Tu, Jesus!".to_string(),
                forced_break: false,
                duration_secs: None,
            },
            SongSlide {
                id: 0,
                song_id: 0,
                order: 1,
                text: "Vem de todo o mal salvar\nQuem em Ti se refugiar.".to_string(),
                forced_break: false,
                duration_secs: None,
            },
        ];
        let SongInsertOutcome::Inserted { id: song_id, .. } =
            storage.insert_song(&song, false).await?
        else {
            panic!("insert song");
        };

        let engine = StateEngine::new();
        let ctrl_state = ControlState::new(engine.clone(), storage.clone(), "secret-culto-token");
        let handle = start_control_server("127.0.0.1:0".parse()?, ctrl_state).await?;
        let base_url = format!("http://{}", handle.bound_addr);

        let client = reqwest::Client::new();

        // 1. Verify mobile HTML loads on LAN
        let html_resp = client.get(&base_url).send().await?;
        assert_eq!(html_resp.status(), StatusCode::OK);
        assert!(html_resp.text().await?.contains("OVERTURE REMOTE"));

        // 2. Verify missing or wrong token returns 401 Unauthorized
        let unauth = client.get(format!("{base_url}/api/state")).send().await?;
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // 3. Connect real-time WebSocket client with valid token
        let ws_url = format!("ws://{}/api/ws?token=secret-culto-token", handle.bound_addr);
        let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url).await?;

        // Drain initial snapshot frame
        let first_msg = tokio::time::timeout(Duration::from_secs(2), ws_stream.next())
            .await?
            .expect("ws stream open")?;
        assert!(first_msg.into_text()?.contains("output_updated"));

        // 4. Present song from mobile phone HTTP API
        let pres_resp = client
            .post(format!("{base_url}/api/songs/{song_id}/present"))
            .header("X-Overture-Token", "secret-culto-token")
            .send()
            .await?;
        assert_eq!(pres_resp.status(), StatusCode::OK);
        assert_eq!(engine.snapshot().current_slide_index, 0);

        // 5. Advance to Next Slide from mobile phone (`POST /api/next`)
        let next_resp = client
            .post(format!("{base_url}/api/next"))
            .header("Authorization", "Bearer secret-culto-token")
            .send()
            .await?;
        assert_eq!(next_resp.status(), StatusCode::OK);
        let st_after: OutputState = next_resp.json().await?;
        assert_eq!(st_after.current_slide_index, 1);
        assert_eq!(engine.snapshot().current_slide_index, 1);

        // 6. Verify real-time WebSocket received the slide-1 update
        let mut saw_slide_1_on_ws = false;
        for _ in 0..4 {
            if let Ok(Some(Ok(msg))) =
                tokio::time::timeout(Duration::from_millis(500), ws_stream.next()).await
                && let Ok(txt) = msg.into_text()
                && txt.contains("\"current_slide_index\":1")
            {
                saw_slide_1_on_ws = true;
                break;
            }
        }
        assert!(saw_slide_1_on_ws, "WebSocket must stream slide change in real time");

        handle.shutdown();
        Ok(())
    }
}
