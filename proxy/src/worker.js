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
// It also serves the web version of the screen: the page itself (static assets from
// the Idleview-Web submodule, see wrangler.toml) and GET /api/view, which returns the finished view for the
// visitor's location and local time. The view and the photo search are computed by
// idleview-core, the same Rust as the desktop app, compiled to WebAssembly (core.js).
// Visitors cannot choose a search, so the web adds no way to spend the quota.
//
// No CORS headers: the page is served from this same origin, and the app calls from Rust.

import QUERIES from './queries.json'
import * as core from './core.js'

const ALLOWED_QUERIES = new Set(QUERIES)
const BATCH_SIZE = 10
const CACHE_SECONDS = 30 * 60
/** How long every web visitor sees the same photo for a search. */
const WEB_PHOTO_SLOT_MS = 30 * 60 * 1000
const WEATHER_CACHE_SECONDS = 15 * 60

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
      if (url.pathname === '/api/view' && request.method === 'GET') {
        return await handleView(request, env, ctx)
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

  const { batch, error } = await photoBatch(query, env, ctx)
  if (error) return error
  return json(batch[Math.floor(Math.random() * batch.length)], 200)
}

/**
 * The cached batch of photos for an allowed search, fetching it from Unsplash (one
 * call, BATCH_SIZE photos) when the edge cache is empty. Returns { batch } or
 * { error: Response }.
 */
async function photoBatch(query, env, ctx) {
  const cache = caches.default
  const cacheKey = new Request(`https://photo-batches.idleview.internal/${encodeURIComponent(query)}`)

  const cached = await cache.match(cacheKey)
  if (cached) return { batch: await cached.json() }

  if (!env.UNSPLASH_ACCESS_KEY) {
    return { error: json({ error: 'Server is missing UNSPLASH_ACCESS_KEY' }, 500) }
  }

  const unsplashUrl =
    `https://api.unsplash.com/photos/random?orientation=landscape&count=${BATCH_SIZE}&query=${encodeURIComponent(query)}`
  const res = await fetch(unsplashUrl, {
    headers: { Authorization: `Client-ID ${env.UNSPLASH_ACCESS_KEY}`, 'Accept-Version': 'v1' }
  })

  // Unsplash rate-limited *us*. Report it as retryable so the app keeps its photo.
  if (res.status === 403 || res.status === 429) {
    return { error: json({ error: 'Photo service is over its quota, try again later' }, 503) }
  }
  if (!res.ok) {
    return { error: json({ error: `Unsplash error (${res.status})` }, 502) }
  }

  const data = await res.json()
  // The app sizes raw_url itself (src-tauri/src/photos.rs); the web page's size is
  // applied in sizedPhotoUrl below.
  const batch = (Array.isArray(data) ? data : [data])
    .map(photo => ({
      raw_url: photo?.urls?.regular || '',
      author: photo?.user?.name || 'Unknown',
      author_url: photo?.user?.links?.html || 'https://unsplash.com',
      download_location: photo?.links?.download_location || ''
    }))
    .filter(photo => photo.raw_url)

  if (!batch.length) {
    return { error: json({ error: 'No photos found for this search' }, 502) }
  }

  const toCache = new Response(JSON.stringify(batch), {
    headers: { 'Content-Type': 'application/json', 'Cache-Control': `max-age=${CACHE_SECONDS}` }
  })
  ctx.waitUntil(cache.put(cacheKey, toCache))
  return { batch }
}

// ===== Web version =====

/**
 * GET /api/view?w=&h= - everything the web page draws, for the visitor's location and
 * local time (both from Cloudflare's request.cf, so the page asks for nothing).
 *
 * A failure in weather or photos never fails the view: the page still gets its clock.
 */
export async function handleView(request, env, ctx, cf = request.cf || {}) {
  const url = new URL(request.url)
  const timeZone = validTimeZone(cf.timezone)
  const now = localTime(new Date(), timeZone)
  const latitude = Number.parseFloat(cf.latitude)
  const longitude = Number.parseFloat(cf.longitude)
  const located = Number.isFinite(latitude) && Number.isFinite(longitude)

  const weather = located ? await cachedWeather(latitude, longitude, ctx).catch(() => null) : null
  const query = core.photoQuery({ now, weather })

  let photo = null
  let downloadLocation = ''
  const { batch } = await photoBatch(query, env, ctx)
  if (batch) {
    // The same photo for every visitor during a slot: steady on screen, and cacheable.
    const chosen = batch[Math.floor(Date.now() / WEB_PHOTO_SLOT_MS) % batch.length]
    photo = {
      url: sizedPhotoUrl(chosen.raw_url, url.searchParams.get('w'), url.searchParams.get('h')),
      author: chosen.author,
      author_url: chosen.author_url
    }
    downloadLocation = chosen.download_location
  }

  const view = core.view({
    now,
    units: unitsFor(cf.country),
    location: cf.city || null,
    weather,
    photo
  })
  return json({ view, download_location: downloadLocation }, 200)
}

/** The web page has no settings, so the one sensible guess: the US reads °F, mph, 12h. */
export function unitsFor(country) {
  return country === 'US'
    ? { temperature_unit: 'fahrenheit', time_format: '12h', date_format: 'mdy', wind_speed_unit: 'mph' }
    : {}
}

function validTimeZone(timeZone) {
  try {
    new Intl.DateTimeFormat('en-US', { timeZone })
    return timeZone || 'UTC'
  } catch {
    return 'UTC'
  }
}

/** Wall-clock time in `timeZone` as the core expects it: "2026-04-26T10:42:05". */
export function localTime(date, timeZone) {
  const parts = Object.fromEntries(
    new Intl.DateTimeFormat('en-US', {
      timeZone,
      hourCycle: 'h23',
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit'
    })
      .formatToParts(date)
      .map(part => [part.type, part.value])
  )
  return `${parts.year}-${parts.month}-${parts.day}T${parts.hour}:${parts.minute}:${parts.second}`
}

/**
 * Current weather from Open-Meteo, cached per 0.1° square (about 11 km) for 15 minutes.
 * Every web visitor's request leaves from this Worker, so without the cache they would
 * all count against one free-tier allowance.
 */
async function cachedWeather(latitude, longitude, ctx) {
  const lat = latitude.toFixed(1)
  const lon = longitude.toFixed(1)
  const cache = caches.default
  const cacheKey = new Request(`https://weather.idleview.internal/${lat},${lon}`)

  const cached = await cache.match(cacheKey)
  if (cached) return cached.json()

  const res = await fetch(
    `https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}` +
      '&current=temperature_2m,relative_humidity_2m,rain,showers,snowfall,cloudcover,wind_speed_10m,weathercode' +
      '&daily=sunrise,sunset&timezone=auto'
  )
  if (!res.ok) throw new Error(`Open-Meteo error (${res.status})`)
  const data = await res.json()

  // Open-Meteo gives local times without seconds ("2026-04-26T06:13").
  const withSeconds = (time) => (typeof time === 'string' && time.length === 16 ? `${time}:00` : null)
  const weather = {
    temperature_c: data.current.temperature_2m,
    humidity: data.current.relative_humidity_2m,
    wind_kmh: data.current.wind_speed_10m,
    cloudcover: data.current.cloudcover,
    rain_mm: data.current.rain,
    showers_mm: data.current.showers,
    snowfall_cm: data.current.snowfall,
    weathercode: data.current.weathercode,
    sunrise: withSeconds(data.daily?.sunrise?.[0]),
    sunset: withSeconds(data.daily?.sunset?.[0])
  }

  ctx.waitUntil(cache.put(cacheKey, new Response(JSON.stringify(weather), {
    headers: { 'Content-Type': 'application/json', 'Cache-Control': `max-age=${WEATHER_CACHE_SECONDS}` }
  })))
  return weather
}

/**
 * Size an Unsplash CDN URL for the visitor's screen: our w/h/fit/q replace Unsplash's,
 * its identity params stay. The desktop app does the same in Rust (photos.rs).
 */
export function sizedPhotoUrl(rawUrl, width, height) {
  const clamp = (value, fallback) => {
    const n = Number.parseInt(value, 10)
    return Number.isFinite(n) ? Math.min(3840, Math.max(320, n)) : fallback
  }
  const url = new URL(rawUrl)
  for (const key of ['w', 'h', 'fit', 'q']) url.searchParams.delete(key)
  url.searchParams.set('w', String(clamp(width, 1920)))
  url.searchParams.set('h', String(clamp(height, 1080)))
  url.searchParams.set('fit', 'crop')
  url.searchParams.set('q', '80')
  return url.toString()
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
