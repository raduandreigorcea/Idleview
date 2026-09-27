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
// Anyone can call this worker, so it is built to be safe when they do:
//   - Only searches on queries.json are accepted, so the key cannot be spent on
//     arbitrary searches. (That list is what idleview-core's photo_query produces; a
//     Rust test keeps the two identical.)
//   - Each search costs one Unsplash call per 30 minutes per Cloudflare location,
//     however many screens ask: the call fetches a batch of photos, which is cached at
//     the edge and served from. (The edge cache is per data centre; KV would make it
//     global if the quota ever gets tight.)
//   - A per-IP rate limit backs both up.
//
// No CORS headers: the app calls this from Rust, not from a browser.

import QUERIES from './queries.json'

const ALLOWED_QUERIES = new Set(QUERIES)
const BATCH_SIZE = 10
const CACHE_SECONDS = 30 * 60

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url)

    try {
      if (!(await withinRateLimit(request, env))) {
        return json({ error: 'Too many requests, slow down' }, 429)
      }
      if (url.pathname === '/api/photo' && request.method === 'GET') {
        return await handlePhoto(url, env, ctx)
      }
      if (url.pathname === '/api/photo/download' && request.method === 'POST') {
        return await handleDownload(request, env)
      }
      if (url.pathname === '/api/health' && request.method === 'GET') {
        return json({ status: 'healthy' }, 200)
      }
      return json({ error: 'Not found' }, 404)
    } catch (error) {
      return json({ error: error?.message || 'Unexpected error' }, 500)
    }
  }
}

// Not bound in tests or a bare `wrangler dev`, where there is nothing to protect.
async function withinRateLimit(request, env) {
  if (!env.LIMITER) return true
  const ip = request.headers.get('CF-Connecting-IP') || 'unknown'
  const { success } = await env.LIMITER.limit({ key: ip })
  return success
}

async function handlePhoto(url, env, ctx) {
  const query = (url.searchParams.get('query') || '').trim().toLowerCase()
  if (!ALLOWED_QUERIES.has(query)) {
    return json({ error: 'Unknown photo search' }, 400)
  }

  const cache = caches.default
  const cacheKey = new Request(`https://photo-batches.idleview.internal/${encodeURIComponent(query)}`)

  let batch = null
  const cached = await cache.match(cacheKey)
  if (cached) batch = await cached.json()

  if (!batch) {
    if (!env.UNSPLASH_ACCESS_KEY) {
      return json({ error: 'Server is missing UNSPLASH_ACCESS_KEY' }, 500)
    }

    const unsplashUrl =
      `https://api.unsplash.com/photos/random?orientation=landscape&count=${BATCH_SIZE}&query=${encodeURIComponent(query)}`
    const res = await fetch(unsplashUrl, {
      headers: { Authorization: `Client-ID ${env.UNSPLASH_ACCESS_KEY}`, 'Accept-Version': 'v1' }
    })

    // Unsplash rate-limited *us*. Report it as retryable so the app keeps its photo.
    if (res.status === 403 || res.status === 429) {
      return json({ error: 'Photo service is over its quota, try again later' }, 503)
    }
    if (!res.ok) {
      return json({ error: `Unsplash error (${res.status})` }, 502)
    }

    const data = await res.json()
    // The app sizes raw_url itself (src-tauri/src/photos.rs), so that stays in one place.
    batch = (Array.isArray(data) ? data : [data])
      .map(photo => ({
        raw_url: photo?.urls?.regular || '',
        author: photo?.user?.name || 'Unknown',
        author_url: photo?.user?.links?.html || 'https://unsplash.com',
        download_location: photo?.links?.download_location || ''
      }))
      .filter(photo => photo.raw_url)

    if (!batch.length) {
      return json({ error: 'No photos found for this search' }, 502)
    }

    const toCache = new Response(JSON.stringify(batch), {
      headers: { 'Content-Type': 'application/json', 'Cache-Control': `max-age=${CACHE_SECONDS}` }
    })
    ctx.waitUntil(cache.put(cacheKey, toCache))
  }

  return json(batch[Math.floor(Math.random() * batch.length)], 200)
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
