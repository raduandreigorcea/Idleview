use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::{info, error};

use crate::settings_manager::{Settings, SettingsManager};

/// Current photo information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentPhoto {
    pub url: String,
    pub author: String,
    pub author_url: String,
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub settings_manager: SettingsManager,
    pub app_handle: tauri::AppHandle,
    pub current_photo: Arc<Mutex<Option<CurrentPhoto>>>,
}

/// Custom error type for HTTP responses
pub struct AppError(String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": self.0
        });
        (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: std::error::Error,
{
    fn from(err: E) -> Self {
        AppError(err.to_string())
    }
}

/// GET /api/settings - Return current settings as JSON
async fn get_settings(State(state): State<AppState>) -> Result<Json<Settings>, AppError> {
    match state.settings_manager.get() {
        Ok(settings) => Ok(Json(settings)),
        Err(e) => {
            error!("Failed to get settings: {}", e);
            Err(AppError(e))
        }
    }
}

/// PUT /api/settings - Update all settings from JSON body
async fn update_settings(
    State(state): State<AppState>,
    Json(settings): Json<Settings>,
) -> Result<Json<Settings>, AppError> {
    match state.settings_manager.update_all(settings.clone()) {
        Ok(_) => {
            info!("Settings updated successfully");
            // Emit event to Tauri window
            let _ = state.app_handle.emit("settings-updated", &settings);
            Ok(Json(settings))
        }
        Err(e) => {
            error!("Failed to update settings: {}", e);
            Err(AppError(e))
        }
    }
}

/// PATCH /api/settings - Partially update settings from JSON body
async fn patch_settings(
    State(state): State<AppState>,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<Settings>, AppError> {
    match state.settings_manager.update_partial(updates) {
        Ok(settings) => {
            info!("Settings partially updated successfully");
            // Emit event to Tauri window
            let _ = state.app_handle.emit("settings-updated", &settings);
            Ok(Json(settings))
        }
        Err(e) => {
            error!("Failed to partially update settings: {}", e);
            Err(AppError(e))
        }
    }
}

/// POST /api/settings/reset - Reset all settings to defaults
async fn reset_settings(State(state): State<AppState>) -> Result<Json<Settings>, AppError> {
    let default_settings = Settings::default();
    
    match state.settings_manager.update_all(default_settings.clone()) {
        Ok(_) => {
            info!("Settings reset to defaults successfully");
            // Emit event to Tauri window
            let _ = state.app_handle.emit("settings-updated", &default_settings);
            Ok(Json(default_settings))
        }
        Err(e) => {
            error!("Failed to reset settings: {}", e);
            Err(AppError(e))
        }
    }
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "service": "idleview-api"
    }))
}

/// GET /api/photo/current - Return current photo information
async fn get_current_photo(State(state): State<AppState>) -> Result<Json<Option<CurrentPhoto>>, AppError> {
    let photo = state.current_photo
        .lock()
        .map_err(|e| AppError(format!("Failed to lock photo state: {}", e)))?;
    Ok(Json(photo.clone()))
}

/// POST /api/photo/current - Update current photo information
async fn update_current_photo(
    State(state): State<AppState>,
    Json(photo): Json<CurrentPhoto>,
) -> Result<Json<CurrentPhoto>, AppError> {
    let mut current = state.current_photo
        .lock()
        .map_err(|e| AppError(format!("Failed to lock photo state: {}", e)))?;
    *current = Some(photo.clone());
    info!("Current photo updated: {} by {}", photo.url, photo.author);
    Ok(Json(photo))
}

/// Create the router with all routes
fn create_router(state: AppState, static_dir: PathBuf) -> Router {
    // API routes
    let api_routes = Router::new()
        .route("/settings", get(get_settings))
        .route("/settings", put(update_settings))
        .route("/settings", patch(patch_settings))
        .route("/settings/reset", post(reset_settings))
        .route("/photo/current", get(get_current_photo))
        .route("/photo/current", post(update_current_photo))
        .route("/health", get(health_check));

    // CORS configuration - allow all origins for development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build the main router
    Router::new()
        .nest("/api", api_routes)
        .nest_service("/", ServeDir::new(static_dir))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors),
        )
        .with_state(state)
}

/// Get local IP addresses for display
fn get_local_ips() -> Vec<String> {
    let mut ips = vec!["127.0.0.1".to_string()];
    
    if let Ok(local_ip) = local_ip_address::local_ip() {
        ips.push(local_ip.to_string());
    }
    
    ips
}

/// Start the HTTP server
pub async fn start_server(port: u16, app_handle: tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Initialize settings manager
    let settings_manager = SettingsManager::new()
        .map_err(|e| format!("Failed to initialize settings manager: {}", e))?;

    let state = AppState { 
        settings_manager,
        app_handle: app_handle.clone(),
        current_photo: Arc::new(Mutex::new(None)),
    };

    // Determine static files directory
    let static_dir = if cfg!(debug_assertions) {
        // Development: use the idleview-control folder
        PathBuf::from("../idleview-control")
    } else {
        // Production: use resource path bundled with the app
        app_handle
            .path()
            .resource_dir()
            .expect("Failed to get resource directory")
            .join("idleview-control")
    };

    // Create router
    let app = create_router(state, static_dir);

    // Bind to 0.0.0.0 to accept connections from local network
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    info!("🚀 Idleview HTTP Server starting...");
    info!("📍 Server listening on port {}", port);
    
    let ips = get_local_ips();
    info!("🌐 Access the control panel at:");
    for ip in ips {
        info!("   http://{}:{}", ip, port);
    }
    
    info!("📡 API endpoints available at:");
    info!("   GET    /api/settings");
    info!("   PUT    /api/settings");
    info!("   PATCH  /api/settings");
    info!("   POST   /api/settings/reset");
    info!("   GET    /api/photo/current");
    info!("   POST   /api/photo/current");
    info!("   GET    /api/health");

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e).into())
}

// Tests disabled - requires tauri AppHandle which can't be easily mocked
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use axum::body::Body;
//     use axum::http::{Request, StatusCode};
//     use tower::ServiceExt;
//
//     #[tokio::test]
//     async fn test_health_check() {
//         let settings_manager = SettingsManager::new().unwrap();
//         let state = AppState { settings_manager };
//         let app = create_router(state, PathBuf::from("dist"));
//
//         let response = app
//             .oneshot(
//                 Request::builder()
//                     .uri("/api/health")
//                     .body(Body::empty())
//                     .unwrap(),
//             )
//             .await
//             .unwrap();
//
//         assert_eq!(response.status(), StatusCode::OK);
//     }
// }
