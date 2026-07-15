// Idleview photo proxy — a Cloudflare Worker that holds the Unsplash key.
//
// The desktop app has no key. It calls this worker; this worker calls Unsplash with the
// key, which lives here as a Wrangler secret (`wrangler secret put UNSPLASH_ACCESS_KEY`)
// and is never in the repo or in anything that ships. A key on a user's machine -
// compiled in or in a settings file - is recoverable from the installer, so it has to
// live server-side.
//
// Two endpoints, matching what src-tauri/src/photos.rs calls:
//   GET  /api/photo?query=...          -> { raw_url, author, author_url, download_location }
//   POST /api/photo/download {downloadUrl} -> { ok }
//
// No CORS headers on purpose: the app calls this server-side (not from a browser), so no
// cross-origin grant is needed, and withholding one keeps random web pages from spending
// your Unsplash quota.

export default {
  async fetch(request, env) {
    const url = new URL(request.url)

    try {
      if (url.pathname === '/api/photo' && request.method === 'GET') {
        return await handlePhoto(url, env)
      }
      if (url.pathname === '/api/photo/download' && request.method === 'POST') {
        return await handleDownload(request, env)
      }
      if (url.pathname === '/api/health' && request.method === 'GET') {
        return json({ status: 'healthy', key: env.UNSPLASH_ACCESS_KEY ? 'set' : 'missing' }, 200)
      }
      return json({ error: 'Not found' }, 404)
    } catch (error) {
      return json({ error: error?.message || 'Unexpected error' }, 500)
    }
  }
}

const MAX_QUERY_LENGTH = 120

async function handlePhoto(url, env) {
  if (!env.UNSPLASH_ACCESS_KEY) {
    return json({ error: 'Server is missing UNSPLASH_ACCESS_KEY' }, 500)
  }

  const query = (url.searchParams.get('query') || 'nature').trim().slice(0, MAX_QUERY_LENGTH)

  const unsplashUrl =
    `https://api.unsplash.com/photos/random?orientation=landscape&query=${encodeURIComponent(query)}`
  const res = await fetch(unsplashUrl, {
    headers: { Authorization: `Client-ID ${env.UNSPLASH_ACCESS_KEY}`, 'Accept-Version': 'v1' }
  })

  // Unsplash rate-limited *us*. Report it as a distinct, retryable condition so the app
  // falls back to its cached photo instead of treating the query as broken.
  if (res.status === 403 || res.status === 429) {
    return json({ error: 'Photo service is over its quota, try again later' }, 503)
  }
  if (!res.ok) {
    const details = await res.text().catch(() => '')
    return json({ error: `Unsplash error (${res.status})`, details }, 502)
  }

  const data = await res.json()

  // The app appends its own sizing/quality params to raw_url, so that logic stays in one
  // place (src-tauri/src/lib.rs::build_photo_url) rather than being duplicated here.
  return json({
    raw_url: data?.urls?.regular || '',
    author: data?.user?.name || 'Unknown',
    author_url: data?.user?.links?.html || 'https://unsplash.com',
    download_location: data?.links?.download_location || ''
  }, 200)
}

// SECURITY: downloadUrl comes from the client and we fetch it with OUR key attached.
// Without an exact allowlist this endpoint is a credentialed SSRF gadget - anyone could
// point it at their own server and read the Authorization header, i.e. steal the key
// this worker exists to hide. Match the host exactly and the path shape strictly; never
// relax this into a substring check (api.unsplash.com.evil.test would slip through).
export function isUnsplashDownloadUrl(raw) {
  let url
  try {
    url = new URL(raw)
  } catch {
    return false
  }
  if (url.protocol !== 'https:') return false
  if (url.hostname !== 'api.unsplash.com') return false
  if (url.port && url.port !== '443') return false
  return /^\/photos\/[A-Za-z0-9_-]+\/download\/?$/.test(url.pathname)
}

async function handleDownload(request, env) {
  const body = await request.json().catch(() => ({}))
  const downloadUrl = body?.downloadUrl || ''

  if (!downloadUrl || typeof downloadUrl !== 'string') {
    return json({ ok: false, error: 'downloadUrl is required' }, 400)
  }
  if (!isUnsplashDownloadUrl(downloadUrl)) {
    return json({ ok: false, error: 'Not an Unsplash download location' }, 400)
  }

  // Unsplash's terms require this attribution ping when a photo is used. Best-effort: a
  // failed ping must never stop the app showing the photo it already has.
  await fetch(downloadUrl, {
    headers: { Authorization: `Client-ID ${env.UNSPLASH_ACCESS_KEY}` }
  }).catch(() => null)

  return json({ ok: true }, 200)
}

function json(payload, status) {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }
  })
}
