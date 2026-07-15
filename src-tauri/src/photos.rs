//! Where photos come from.
//!
//! Users have no Unsplash key and are never asked for one - and neither is a developer.
//! Every build, dev or release, fetches through a proxy that holds the key server-side
//! (see the `proxy/` directory). The key is therefore never present on any machine that
//! runs this app: not compiled in, not in a settings file, not read from the environment.
//!
//! To test against a local worker, point `IDLEVIEW_PHOTO_PROXY` at a `wrangler dev`
//! instance; otherwise `DEFAULT_PHOTO_PROXY` is used.

use serde::Deserialize;

/// Where the deployed proxy lives.
///
/// This is the Cloudflare Worker in `proxy/`, which holds the Unsplash key as a Wrangler
/// secret. Set this to your worker's real `*.workers.dev` URL (printed by
/// `wrangler deploy`) before shipping a build.
///
/// Override at runtime with `IDLEVIEW_PHOTO_PROXY` (handy against a local `wrangler dev`).
pub const DEFAULT_PHOTO_PROXY: &str = "https://idleview-photos.idleview.workers.dev";

/// The photo fields the app actually uses, in the shape the proxy normalises to.
#[derive(Debug, Clone)]
pub struct SourcePhoto {
    pub raw_url: String,
    pub author: String,
    pub author_url: String,
    pub download_location: String,
}

pub fn proxy_base() -> String {
    std::env::var("IDLEVIEW_PHOTO_PROXY")
        .ok()
        .map(|base| base.trim().trim_end_matches('/').to_string())
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| DEFAULT_PHOTO_PROXY.to_string())
}

// ----- Proxy responses -----

#[derive(Debug, Deserialize)]
struct ProxyPhoto {
    // The worker calls this field `url`; `raw_url` is the clearer name for "the base URL
    // the app then sizes itself". The alias lets us read the currently-deployed worker
    // as-is, and a future worker that adopts `raw_url`, without a flag day.
    #[serde(alias = "url")]
    raw_url: String,
    author: String,
    author_url: String,
    #[serde(default)]
    download_location: String,
}

#[derive(Debug, Deserialize)]
struct ProxyError {
    error: String,
}

pub async fn fetch_photo(client: &reqwest::Client, query: &str) -> Result<SourcePhoto, String> {
    let url = format!(
        "{}/api/photo?query={}",
        proxy_base(),
        urlencoding::encode(query)
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Could not reach the photo service: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        // The proxy sends a human-readable reason (over quota, bad query). Prefer it to
        // a bare status code, since this string can end up in the debug panel.
        let message = response
            .json::<ProxyError>()
            .await
            .map(|body| body.error)
            .unwrap_or_else(|_| format!("Photo service error ({})", status));
        return Err(message);
    }

    let photo: ProxyPhoto = response
        .json()
        .await
        .map_err(|e| format!("Unreadable response from the photo service: {}", e))?;

    Ok(SourcePhoto {
        raw_url: photo.raw_url,
        author: photo.author,
        author_url: photo.author_url,
        download_location: photo.download_location,
    })
}

/// Unsplash's terms require a ping to `download_location` whenever a photo is actually
/// used. Best-effort: a failed attribution ping must never stop the photo being shown.
pub async fn trigger_download(client: &reqwest::Client, download_location: &str) {
    if download_location.trim().is_empty() {
        return;
    }

    // The worker's contract: POST /api/photo/download with a JSON body. It re-checks the
    // URL against Unsplash before attaching the key, so this stays a courtesy ping and
    // never a way to point the key at an arbitrary host.
    let url = format!("{}/api/photo/download", proxy_base());
    let result = client
        .post(&url)
        .json(&serde_json::json!({ "downloadUrl": download_location }))
        .send()
        .await;

    if let Err(e) = result {
        eprintln!("Attribution ping failed (photo still shown): {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proxy_base_has_no_trailing_slash_to_double_up_on() {
        // Callers append "/api/photo", so a trailing slash would produce "//api/photo".
        assert!(!proxy_base().ends_with('/'));
    }
}
