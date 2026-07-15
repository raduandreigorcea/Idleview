use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::convert::Infallible;
use tauri::{Emitter, Manager};
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::{info, error};
use tokio::sync::broadcast;
use futures::stream::Stream;
use async_stream::stream;

use crate::settings_manager::{self, Settings, SettingsManager};

/// Header carrying the shared token that authorises a write.
const TOKEN_HEADER: &str = "x-idleview-token";
/// Header identifying which panel made a change, so that panel can recognise the
/// resulting broadcast as its own echo and skip reloading.
const CLIENT_HEADER: &str = "x-idleview-client";

/// Current photo information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentPhoto {
    pub url: String,
    pub author: String,
    pub author_url: String,
}

/// The currently displayed photo plus the SSE fan-out channel. Shared between the
/// HTTP handlers and the Tauri command the dashboard calls, so both publish photo
/// changes the same way.
#[derive(Clone)]
pub struct PhotoChannel {
    current: Arc<Mutex<Option<CurrentPhoto>>>,
    events: broadcast::Sender<String>,
}

impl PhotoChannel {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(100);
        Self {
            current: Arc::new(Mutex::new(None)),
            events,
        }
    }

    pub fn get(&self) -> Result<Option<CurrentPhoto>, String> {
        self.current
            .lock()
            .map(|photo| photo.clone())
            .map_err(|e| format!("Failed to lock photo state: {}", e))
    }

    pub fn set(&self, photo: CurrentPhoto) -> Result<(), String> {
        {
            let mut current = self
                .current
                .lock()
                .map_err(|e| format!("Failed to lock photo state: {}", e))?;
            *current = Some(photo.clone());
        }
        info!("Current photo updated: {} by {}", photo.url, photo.author);
        self.broadcast(&json!({ "type": "photo-updated", "photo": photo }));
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.events.subscribe()
    }

    pub fn broadcast(&self, event: &serde_json::Value) {
        let _ = self
            .events
            .send(serde_json::to_string(event).unwrap_or_default());
    }
}

impl Default for PhotoChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub settings_manager: SettingsManager,
    pub app_handle: tauri::AppHandle,
    pub photos: PhotoChannel,
}

/// An error with the status it should be reported as. Defaults to 500 so an
/// unexpected failure never accidentally reads as success.
pub struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn internal(message: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: message.into() }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "Missing or invalid control token".to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: std::error::Error,
{
    fn from(err: E) -> Self {
        AppError::internal(err.to_string())
    }
}

/// Reject a request that does not carry the control token.
///
/// The server binds 0.0.0.0 so a phone on the same network can reach it, which
/// means anything else on that network can reach it too. CORS does not help here:
/// it is a browser policy, not access control, and does nothing about `curl`.
///
/// EVERY mutating handler must call this first. Reads are deliberately open - they
/// expose nothing the screen is not already displaying to the room.
fn authorize(headers: &HeaderMap) -> Result<(), AppError> {
    let provided = headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    match settings_manager::token_matches(provided) {
        Ok(true) => Ok(()),
        Ok(false) => Err(AppError::unauthorized()),
        Err(e) => Err(AppError::internal(e)),
    }
}

/// Which panel sent this request, if it said. Echoed back in the broadcast so that
/// panel can ignore its own change instead of reloading in a loop.
fn client_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CLIENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

/// Announce a settings change to everyone listening: the Tauri window via its event
/// channel, and any browser control panels via SSE. Every mutating handler must call
/// it - a handler that skips it leaves other open panels stale.
///
/// The payload is the redacted view. Secrets must never reach an SSE subscriber.
fn notify_settings_changed(state: &AppState, settings: &Settings, origin: Option<String>) {
    let redacted = settings.redacted();
    let _ = state.app_handle.emit("settings-updated", &redacted);
    state.photos.broadcast(&json!({
        "type": "settings-updated",
        "settings": redacted,
        "origin": origin,
    }));
}

/// GET /api/settings - current settings, minus anything secret
async fn get_settings(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state
        .settings_manager
        .get()
        .map(|settings| Json(settings.redacted()))
        .map_err(|e| {
            error!("Failed to get settings: {}", e);
            AppError::internal(e)
        })
}

/// PUT /api/settings - replace all settings with the JSON body
async fn update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(new_settings): Json<Settings>,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;

    let saved = state.settings_manager.update_all(new_settings).map_err(|e| {
        error!("Failed to replace settings: {}", e);
        AppError::internal(e)
    })?;

    info!("Settings replaced successfully");
    notify_settings_changed(&state, &saved, client_id(&headers));
    Ok(Json(saved.redacted()))
}

/// PATCH /api/settings - merge a partial JSON body into the current settings
async fn patch_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;

    let saved = state.settings_manager.update_partial(updates).map_err(|e| {
        error!("Failed to partially update settings: {}", e);
        AppError::internal(e)
    })?;

    info!("Settings partially updated successfully");
    notify_settings_changed(&state, &saved, client_id(&headers));
    Ok(Json(saved.redacted()))
}

/// POST /api/settings/reset - reset all settings to defaults
async fn reset_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;

    let saved = state
        .settings_manager
        .update_all(Settings::default())
        .map_err(|e| {
            error!("Failed to reset settings: {}", e);
            AppError::internal(e)
        })?;

    info!("Settings reset to defaults successfully");
    notify_settings_changed(&state, &saved, client_id(&headers));
    Ok(Json(saved.redacted()))
}

/// GET /api/auth/check - does this token work? Lets the panel validate a token the
/// user just typed, instead of finding out on their next edit.
async fn auth_check(headers: HeaderMap) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;
    Ok(Json(json!({ "ok": true })))
}

/// GET /api/fonts - the font catalogue the picker is built from.
///
/// Open, like the other reads. Serving it means the panel cannot offer a font the
/// dashboard will not render, which is what the hand-copied lists kept getting wrong.
async fn get_fonts() -> Json<crate::fonts::FontCatalogue> {
    Json(crate::fonts::catalogue())
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(json!({ "status": "healthy", "service": "idleview-api" }))
}

/// GET /api/photo/current - what the screen is showing right now
async fn get_current_photo(
    State(state): State<AppState>,
) -> Result<Json<Option<CurrentPhoto>>, AppError> {
    state.photos.get().map(Json).map_err(AppError::internal)
}

/// POST /api/photo/current - publish a photo
async fn update_current_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(photo): Json<CurrentPhoto>,
) -> Result<Json<CurrentPhoto>, AppError> {
    authorize(&headers)?;
    state.photos.set(photo.clone()).map_err(AppError::internal)?;
    Ok(Json(photo))
}

/// POST /api/photo/refresh - ask the dashboard for a new photo now.
///
/// The dashboard has always listened for `refresh-photo` (and implemented
/// window.refreshPhoto), but nothing ever emitted the event, so the capability was
/// unreachable from the control panel. This is the missing half.
async fn refresh_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;

    state
        .app_handle
        .emit("refresh-photo", ())
        .map_err(|e| AppError::internal(format!("Failed to request a refresh: {}", e)))?;

    info!("Photo refresh requested by the control panel");
    Ok(Json(json!({ "ok": true })))
}

/// GET /api/events - Server-Sent Events stream for real-time updates
async fn events_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.photos.subscribe();

    let stream = stream! {
        loop {
            match rx.recv().await {
                Ok(event_data) => yield Ok(Event::default().data(event_data)),
                // The client fell behind the 100-event buffer. Skipping is correct:
                // every event carries full state, so the next one resynchronises it.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Create the router with all routes
fn create_router(state: AppState, static_dir: PathBuf) -> Router {
    let api_routes = Router::new()
        .route(
            "/settings",
            get(get_settings).put(update_settings).patch(patch_settings),
        )
        .route("/settings/reset", post(reset_settings))
        .route("/photo/current", get(get_current_photo).post(update_current_photo))
        .route("/photo/refresh", post(refresh_photo))
        .route("/auth/check", get(auth_check))
        .route("/fonts", get(get_fonts))
        .route("/events", get(events_stream))
        .route("/health", get(health_check));

    // No CORS layer on purpose. The control panel is served by this same server, so it
    // is same-origin and needs no grant; the dashboard talks to its own process through
    // Tauri commands rather than over HTTP. That leaves no legitimate cross-origin
    // browser caller, so we grant none.
    //
    // CORS was never the thing protecting this API anyway - it is a browser policy, not
    // access control, and does nothing about a script or `curl` on the LAN. Writes are
    // gated by `authorize` instead.
    Router::new()
        .nest("/api", api_routes)
        .nest_service("/", ServeDir::new(static_dir))
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
        .with_state(state)
}

/// Local IP addresses the control panel can be reached on.
pub fn get_local_ips() -> Vec<String> {
    let mut ips = vec!["127.0.0.1".to_string()];

    if let Ok(local_ip) = local_ip_address::local_ip() {
        ips.push(local_ip.to_string());
    }

    ips
}

/// Start the HTTP server. `photos` is shared with the Tauri command layer so the
/// dashboard and the API publish photo changes through the same channel.
pub async fn start_server(
    port: u16,
    app_handle: tauri::AppHandle,
    photos: PhotoChannel,
) -> Result<(), Box<dyn std::error::Error>> {
    // try_init, not init: init panics if a subscriber is already installed, which
    // would take down the whole app over a logging detail.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .try_init();

    let settings_manager = SettingsManager::new()
        .map_err(|e| format!("Failed to initialize settings manager: {}", e))?;

    // Mint the control token before anything can be asked to authorise against it.
    let token = settings_manager::ensure_auth_token()
        .map_err(|e| format!("Failed to prepare the control token: {}", e))?;

    let state = AppState {
        settings_manager,
        app_handle: app_handle.clone(),
        photos,
    };

    let static_dir = if cfg!(debug_assertions) {
        PathBuf::from("idleview-control")
    } else {
        app_handle
            .path()
            .resource_dir()
            .map_err(|e| format!("Failed to get resource directory: {}", e))?
            .join("idleview-control")
    };

    let app = create_router(state, static_dir);

    // 0.0.0.0 so a phone on the same network can reach the panel. Writes require the
    // token above; reads are open.
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    info!("Idleview HTTP server starting");
    info!("Control panel:");
    for ip in get_local_ips() {
        info!("   http://{}:{}", ip, port);
    }
    info!("Control token (needed to change settings): {}", token);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e).into())
}

// Handler tests need a tauri::AppHandle, which cannot be mocked, so routing is
// exercised by running the app. The pieces that hold no Tauri types - the photo
// channel and the token check - are tested directly here.
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn photo(url: &str) -> CurrentPhoto {
        CurrentPhoto {
            url: url.to_string(),
            author: "Ansel".to_string(),
            author_url: "https://example.com/a".to_string(),
        }
    }

    #[test]
    fn starts_empty_and_stores_the_latest_photo() {
        let photos = PhotoChannel::new();
        assert!(photos.get().unwrap().is_none());

        photos.set(photo("https://example.com/1.jpg")).unwrap();
        assert_eq!(photos.get().unwrap().unwrap().url, "https://example.com/1.jpg");

        photos.set(photo("https://example.com/2.jpg")).unwrap();
        assert_eq!(photos.get().unwrap().unwrap().url, "https://example.com/2.jpg");
    }

    #[test]
    fn setting_a_photo_broadcasts_to_sse_subscribers() {
        let photos = PhotoChannel::new();
        let mut rx = photos.subscribe();

        photos.set(photo("https://example.com/1.jpg")).unwrap();

        let event = rx.try_recv().expect("subscriber should receive the photo event");
        assert!(event.contains("\"type\":\"photo-updated\""));
        assert!(event.contains("https://example.com/1.jpg"));
    }

    #[test]
    fn a_subscriber_that_joins_late_misses_earlier_events() {
        // Documents the broadcast semantics the SSE stream relies on: clients get
        // events from the moment they connect, not a replay of history.
        let photos = PhotoChannel::new();
        photos.set(photo("https://example.com/old.jpg")).unwrap();

        let mut rx = photos.subscribe();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_request_without_a_token_is_rejected() {
        let headers = HeaderMap::new();
        let error = authorize(&headers).expect_err("no token must not authorise");
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn a_request_with_the_wrong_token_is_rejected() {
        let mut headers = HeaderMap::new();
        headers.insert(TOKEN_HEADER, HeaderValue::from_static("NOTTHEONE"));
        let error = authorize(&headers).expect_err("a wrong token must not authorise");
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn a_request_with_the_real_token_is_allowed() {
        let token = settings_manager::ensure_auth_token().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(TOKEN_HEADER, HeaderValue::from_str(&token).unwrap());
        assert!(authorize(&headers).is_ok());
    }

    #[test]
    fn the_client_id_is_read_back_for_echo_suppression() {
        let mut headers = HeaderMap::new();
        assert_eq!(client_id(&headers), None);

        headers.insert(CLIENT_HEADER, HeaderValue::from_static("panel-abc"));
        assert_eq!(client_id(&headers).as_deref(), Some("panel-abc"));
    }
}
