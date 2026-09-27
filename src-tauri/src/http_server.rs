use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::convert::Infallible;
use tauri::Manager;
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::{info, error};
use tokio::sync::broadcast;
use futures::stream::Stream;
use async_stream::stream;

use crate::dashboard::Dashboard;
use crate::library::{self, AddError};
use crate::settings_manager::{self, Settings};
use idleview_core::Photo;

/// Header carrying the shared token that authorises a write.
const TOKEN_HEADER: &str = "x-idleview-token";
/// Header identifying which panel made a change, so that panel can recognise the
/// resulting broadcast as its own echo and skip reloading.
const CLIENT_HEADER: &str = "x-idleview-client";

/// The currently displayed photo plus the SSE fan-out channel. Shared between the
/// HTTP handlers and the Tauri command the dashboard calls, so both publish photo
/// changes the same way.
#[derive(Clone)]
pub struct PhotoChannel {
    current: Arc<Mutex<Option<Photo>>>,
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

    pub fn get(&self) -> Result<Option<Photo>, String> {
        self.current
            .lock()
            .map(|photo| photo.clone())
            .map_err(|e| format!("Failed to lock photo state: {}", e))
    }

    pub fn set(&self, photo: Photo) -> Result<(), String> {
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

    /// Nothing is showing (the user's own library is empty).
    pub fn clear(&self) {
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
        self.broadcast(&json!({ "type": "photo-updated", "photo": null }));
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
    pub dashboard: Dashboard,
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

    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
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

/// Announce a settings change to everyone listening: the dashboard, which re-renders,
/// and any browser control panels via SSE. Every mutating handler must call
/// it - a handler that skips it leaves other open panels stale.
///
/// The payload is the redacted view. Secrets must never reach an SSE subscriber.
fn notify_settings_changed(state: &AppState, settings: &Settings, origin: Option<String>) {
    state.dashboard.wake();
    state.photos.broadcast(&json!({
        "type": "settings-updated",
        "settings": settings.redacted(),
        "origin": origin,
    }));
}

/// GET /api/settings - current settings, minus anything secret
async fn get_settings() -> Result<Json<serde_json::Value>, AppError> {
    settings_manager::read_settings()
        .map(|settings| Json(settings.redacted()))
        .map_err(|e| {
            error!("Failed to get settings: {}", e);
            AppError::internal(e)
        })
}

/// PATCH /api/settings - merge a partial JSON body into the current settings
async fn patch_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;

    let saved = settings_manager::update_settings_partial(updates).map_err(|e| {
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

    let saved = settings_manager::write_settings(&Settings::default())
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

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(json!({ "status": "healthy", "service": "idleview-api" }))
}

/// GET /api/photo/current - what the screen is showing right now
async fn get_current_photo(
    State(state): State<AppState>,
) -> Result<Json<Option<Photo>>, AppError> {
    state.photos.get().map(Json).map_err(AppError::internal)
}

// ===== The user's own photos =====
//
// Unlike settings, even reading these needs the token: they are personal photos, and the
// screen only ever shows one at a time.

/// GET /api/photos - ids of the photos in the library
async fn list_photos(headers: HeaderMap) -> Result<Json<Vec<String>>, AppError> {
    authorize(&headers)?;
    Ok(Json(library::list()))
}

/// GET /api/photos/:id/thumb - a small JPEG for the panel's grid
async fn photo_thumb(headers: HeaderMap, Path(id): Path<String>) -> Result<Response, AppError> {
    authorize(&headers)?;
    let path = library::thumb_path(&id).ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "No such photo"))?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| AppError::new(StatusCode::NOT_FOUND, "No such photo"))?;
    Ok((
        [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=86400")],
        bytes,
    )
        .into_response())
}

/// POST /api/photos - body is one image file (JPEG, PNG or WebP)
async fn upload_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;

    // Decoding and resizing a 12 MP photo is CPU work; keep it off the async threads.
    let id = tokio::task::spawn_blocking(move || library::add(&body))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map_err(|e| match e {
            AddError::NotAnImage => AppError::new(StatusCode::BAD_REQUEST, "That file is not a JPEG, PNG or WebP image"),
            AddError::Full => AppError::new(
                StatusCode::CONFLICT,
                format!("The library is full ({} photos). Remove some first.", library::MAX_PHOTOS),
            ),
            AddError::Storage(e) => AppError::internal(e),
        })?;

    info!("Photo added to the library");
    photos_changed(&state);
    Ok(Json(json!({ "id": id })))
}

/// DELETE /api/photos/:id
async fn delete_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    authorize(&headers)?;
    if !library::remove(&id).map_err(AppError::internal)? {
        return Err(AppError::new(StatusCode::NOT_FOUND, "No such photo"));
    }
    photos_changed(&state);
    Ok(Json(json!({ "ok": true })))
}

/// The screen re-checks its photo (a deleted one must go), and other open panels
/// refresh their grid.
fn photos_changed(state: &AppState) {
    state.dashboard.wake();
    state.photos.broadcast(&json!({ "type": "photos-updated" }));
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
            get(get_settings).patch(patch_settings),
        )
        .route("/settings/reset", post(reset_settings))
        .route("/photo/current", get(get_current_photo))
        .route(
            "/photos",
            get(list_photos)
                .post(upload_photo)
                .layer(DefaultBodyLimit::max(library::MAX_UPLOAD_BYTES)),
        )
        .route("/photos/:id", axum::routing::delete(delete_photo))
        .route("/photos/:id/thumb", get(photo_thumb))
        .route("/auth/check", get(auth_check))
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

/// Start the HTTP server. `photos` is shared with the dashboard, which publishes each
/// new photo through it.
pub async fn start_server(
    port: u16,
    app_handle: tauri::AppHandle,
    photos: PhotoChannel,
    dashboard: Dashboard,
) -> Result<(), Box<dyn std::error::Error>> {
    // try_init, not init: init panics if a subscriber is already installed, which
    // would take down the whole app over a logging detail.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .try_init();

    // Mint the control token before anything can be asked to authorise against it.
    let token = settings_manager::ensure_auth_token()
        .map_err(|e| format!("Failed to prepare the control token: {}", e))?;

    let state = AppState { dashboard, photos };

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

// Handlers are exercised by running the app. The pieces that hold no Tauri types - the photo
// channel and the token check - are tested directly here.
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn photo(url: &str) -> Photo {
        Photo {
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
