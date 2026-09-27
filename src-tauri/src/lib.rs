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
    /// The panel address a phone on this network can reach.
    pub url: String,
    /// A QR code of `url` with the token in its fragment, as an SVG data URL: scanning
    /// it opens the panel already paired.
    pub qr: String,
}

/// The panel address with the token after `#`. A fragment is never sent to a server,
/// so the token stays out of request logs; the panel reads it and then erases it.
pub fn pairing_url(base: &str, token: &str) -> String {
    format!("{base}/#token={token}")
}

/// An SVG data URL, so the page shows it with a plain <img> (no markup injected).
fn qr_data_url(text: &str) -> Result<String, String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .build();
    Ok(format!(
        "data:image/svg+xml;charset=utf-8,{}",
        urlencoding::encode(&svg)
    ))
}

/// What the dashboard needs to show a pairing card: where the control panel lives
/// and the token required to change anything through it.
#[tauri::command]
fn get_server_info() -> Result<ServerInfo, String> {
    let token = settings_manager::ensure_auth_token()?;
    // The LAN address: 127.0.0.1 is useless to the phone being paired.
    let ips = http_server::get_local_ips();
    let ip = ips.iter().find(|ip| *ip != "127.0.0.1").unwrap_or(&ips[0]);
    let url = format!("http://{ip}:{HTTP_PORT}");

    Ok(ServerInfo {
        port: HTTP_PORT,
        qr: qr_data_url(&pairing_url(&url, &token))?,
        token,
        url,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_travels_in_the_fragment_not_the_query() {
        let url = pairing_url("http://192.168.1.130:8737", "K7PMX2QD");
        assert_eq!(url, "http://192.168.1.130:8737/#token=K7PMX2QD");
        assert!(!url.contains('?'), "a query string would reach the server's logs");
    }

    #[test]
    fn the_qr_code_is_an_svg_image() {
        let qr = qr_data_url(&pairing_url("http://192.168.1.130:8737", "K7PMX2QD")).unwrap();
        assert!(qr.starts_with("data:image/svg+xml;charset=utf-8,"));
        let svg = urlencoding::decode(qr.split_once(',').unwrap().1).unwrap();
        assert!(svg.contains("<svg") && svg.contains("</svg>"));
    }
}
