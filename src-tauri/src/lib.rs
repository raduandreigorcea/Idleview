// What the screen shows lives in `idleview_core` (pure, tested); when it updates lives
// in `dashboard`; the control panel's API lives in `http_server`. The webview only draws.
pub mod dashboard;
pub mod http_server;
pub mod library;
pub mod photos;
pub mod settings_manager;

use serde::Serialize;
use tauri::Manager;

/// Port the control-panel HTTP server listens on.
pub const HTTP_PORT: u16 = 8737;

/// The current view, for a webview that loads after the first `view` event was emitted.
#[tauri::command]
fn get_view(dashboard: tauri::State<'_, dashboard::Dashboard>) -> Option<idleview_core::View> {
    dashboard.latest()
}

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub port: u16,
    pub token: String,
    pub urls: Vec<String>,
}

/// What the dashboard needs to show a pairing card: where the control panel lives
/// and the token required to change anything through it.
#[tauri::command]
fn get_server_info() -> Result<ServerInfo, String> {
    Ok(ServerInfo {
        port: HTTP_PORT,
        token: settings_manager::ensure_auth_token()?,
        urls: http_server::get_local_ips()
            .into_iter()
            .map(|ip| format!("http://{}:{}", ip, HTTP_PORT))
            .collect(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env if present, so IDLEVIEW_PHOTO_PROXY can point the app at a local
    // `wrangler dev` worker. The app never holds an Unsplash key.
    let _ = dotenvy::dotenv();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // The webview may read the user's own photos from disk, and nothing else.
            if let Ok(dir) = library::dir() {
                let _ = std::fs::create_dir_all(&dir);
                app.asset_protocol_scope().allow_directory(&dir, true)?;
            }

            // One photo channel, so the control panel sees what the screen shows.
            let photos = http_server::PhotoChannel::new();
            let dashboard = dashboard::start(handle.clone(), photos.clone());
            app.manage(dashboard.clone());

            tauri::async_runtime::spawn(async move {
                if let Err(e) = http_server::start_server(HTTP_PORT, handle, photos, dashboard).await {
                    eprintln!("HTTP server error: {}", e);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_view, get_server_info])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
