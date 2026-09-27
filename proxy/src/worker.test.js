import { describe, it, expect } from 'vitest'
import { isUnsplashDownloadUrl } from './worker.js'

// This guard is the difference between a photo proxy and a credentialed SSRF gadget:
// whatever URL survives it is fetched with our Unsplash key in the Authorization header.
// A miss here leaks the key the worker exists to protect.
describe('download location allowlist', () => {
  it('accepts a genuine Unsplash download location', () => {
    expect(isUnsplashDownloadUrl(
      'https://api.unsplash.com/photos/abc123/download?ixid=M3w4MzE1NDR8MHwxfHJhbmRvbQ'
    )).toBe(true)
    expect(isUnsplashDownloadUrl('https://api.unsplash.com/photos/a-b_C9/download')).toBe(true)
    expect(isUnsplashDownloadUrl('https://api.unsplash.com/photos/abc123/download/')).toBe(true)
  })

  it('refuses to send our key anywhere but Unsplash', () => {
    expect(isUnsplashDownloadUrl('https://api.unsplash.com.evil.test/photos/x/download')).toBe(false)
    expect(isUnsplashDownloadUrl('https://evil.test/photos/x/download?a=api.unsplash.com')).toBe(false)
    expect(isUnsplashDownloadUrl('https://evil.test/api.unsplash.com/photos/x/download')).toBe(false)
    // Userinfo trick: the real host is evil.test.
    expect(isUnsplashDownloadUrl('https://api.unsplash.com@evil.test/photos/x/download')).toBe(false)
  })

  it('refuses other endpoints on Unsplash itself', () => {
    expect(isUnsplashDownloadUrl('https://api.unsplash.com/me')).toBe(false)
    expect(isUnsplashDownloadUrl('https://api.unsplash.com/photos/abc123')).toBe(false)
  })

  it('refuses non-https and internal targets', () => {
    expect(isUnsplashDownloadUrl('http://api.unsplash.com/photos/x/download')).toBe(false)
    expect(isUnsplashDownloadUrl('file:///etc/passwd')).toBe(false)
    expect(isUnsplashDownloadUrl('http://169.254.169.254/latest/meta-data/')).toBe(false)
  })

  it('refuses junk', () => {
    expect(isUnsplashDownloadUrl('')).toBe(false)
    expect(isUnsplashDownloadUrl('not a url')).toBe(false)
    expect(isUnsplashDownloadUrl('//api.unsplash.com/photos/x/download')).toBe(false)
  })
})

import worker from './worker.js'
import QUERIES from './queries.json'

// Stand-ins for the Workers runtime: the edge cache and waitUntil.
function runtime() {
  const store = new Map()
  globalThis.caches = {
    default: {
      match: async (req) => store.get(req.url)?.clone(),
      put: async (req, res) => { store.set(req.url, res) }
    }
  }
  const pending = []
  return { ctx: { waitUntil: (p) => pending.push(p) }, settle: () => Promise.all(pending) }
}

function unsplashStub() {
  const calls = []
  globalThis.fetch = async (url) => {
    calls.push(url)
    const photos = Array.from({ length: 10 }, (_, i) => ({
      urls: { regular: `https://images.unsplash.com/photo-${i}` },
      user: { name: `Author ${i}`, links: { html: 'https://unsplash.com/@a' } },
      links: { download_location: `https://api.unsplash.com/photos/p${i}/download` }
    }))
    return new Response(JSON.stringify(photos), { status: 200 })
  }
  return calls
}

const env = { UNSPLASH_ACCESS_KEY: 'SECRET-KEY' }
const photoRequest = (query) =>
  new Request(`https://proxy.test/api/photo?query=${encodeURIComponent(query)}`)

describe('photo endpoint', () => {
  it('refuses searches the app never makes, without touching Unsplash', async () => {
    const { ctx } = runtime()
    const calls = unsplashStub()

    for (const query of ['cats', '', 'spring night OR anything', 'x'.repeat(500)]) {
      const res = await worker.fetch(photoRequest(query), env, ctx)
      expect(res.status).toBe(400)
    }
    expect(calls).toHaveLength(0)
  })

  it('serves many requests for one search from a single Unsplash call', async () => {
    const { ctx, settle } = runtime()
    const calls = unsplashStub()

    const first = await worker.fetch(photoRequest('autumn rainy night'), env, ctx)
    await settle()
    for (let i = 0; i < 20; i++) {
      const res = await worker.fetch(photoRequest('autumn rainy night'), env, ctx)
      expect(res.status).toBe(200)
    }

    expect(calls).toHaveLength(1)
    expect(calls[0]).toContain('count=10')
    const photo = await first.json()
    expect(photo.raw_url).toMatch(/^https:\/\/images\.unsplash\.com\//)
    expect(photo).toHaveProperty('download_location')
  })

  it('never puts the key in a response', async () => {
    const { ctx } = runtime()
    unsplashStub()

    for (const req of [photoRequest('summer'), new Request('https://proxy.test/api/health')]) {
      const body = await (await worker.fetch(req, env, ctx)).text()
      expect(body).not.toContain('SECRET-KEY')
      expect(body).not.toMatch(/key/i)
    }
  })

  it('turns away a caller over the rate limit', async () => {
    const { ctx } = runtime()
    const calls = unsplashStub()
    const limited = { ...env, LIMITER: { limit: async () => ({ success: false }) } }

    const res = await worker.fetch(photoRequest('summer'), limited, ctx)
    expect(res.status).toBe(429)
    expect(calls).toHaveLength(0)
  })
})

import { handleView, localTime, unitsFor, sizedPhotoUrl } from './worker.js'

describe('web view', () => {
  // Unsplash and Open-Meteo, as the Worker sees them.
  function servicesStub() {
    const calls = []
    globalThis.fetch = async (url) => {
      calls.push(url)
      if (url.includes('api.open-meteo.com')) {
        return new Response(JSON.stringify({
          current: {
            temperature_2m: 18.2, relative_humidity_2m: 50, rain: 0, showers: 0, snowfall: 0,
            cloudcover: 0, wind_speed_10m: 12.3, weathercode: 0
          },
          daily: { sunrise: ['2026-04-26T06:13'], sunset: ['2026-04-26T20:12'] }
        }))
      }
      const photos = Array.from({ length: 10 }, (_, i) => ({
        urls: { regular: `https://images.unsplash.com/photo-${i}?ixid=ABC&w=1080&q=75&fit=max` },
        user: { name: `Author ${i}`, links: { html: 'https://unsplash.com/@a' } },
        links: { download_location: `https://api.unsplash.com/photos/p${i}/download` }
      }))
      return new Response(JSON.stringify(photos))
    }
    return calls
  }

  const bucharest = { city: 'Bucharest', country: 'RO', latitude: '44.43', longitude: '26.10', timezone: 'Europe/Bucharest' }
  const request = (query = '?w=1280&h=800') => new Request(`https://idleview.test/api/view${query}`)

  it('builds the screen for the visitor, from the Rust core', async () => {
    const { ctx } = runtime()
    servicesStub()

    const res = await handleView(request(), env, ctx, bucharest)
    expect(res.status).toBe(200)
    const { view, download_location } = await res.json()

    expect(view.location).toBe('Bucharest')
    expect(view.time).toMatch(/^\d{2}:\d{2}$/) // 24h outside the US
    expect(view.weather).toMatchObject({ temperature: '18 °C', sunrise: '06:13', sunset: '20:12', wind: '12 km/h' })
    expect(view.show.show_clock).toBe(true)
    expect(view.photo.author).toMatch(/^Author \d$/)
    expect(view.photo.url).toContain('w=1280')
    expect(download_location).toMatch(/^https:\/\/api\.unsplash\.com\/photos\/p\d\/download$/)
  })

  it('asks Unsplash only for a search the app itself could make', async () => {
    const { ctx } = runtime()
    const calls = servicesStub()

    await handleView(request(), env, ctx, bucharest)
    const search = new URL(calls.find(url => url.includes('api.unsplash.com'))).searchParams.get('query')
    expect(QUERIES).toContain(search)
  })

  it('shows every visitor the same photo, and fetches weather once per area', async () => {
    const { ctx, settle } = runtime()
    const calls = servicesStub()

    const first = await (await handleView(request(), env, ctx, bucharest)).json()
    await settle()
    const nearby = { ...bucharest, latitude: '44.41', longitude: '26.12' } // same 0.1° square
    const second = await (await handleView(request(), env, ctx, nearby)).json()

    expect(second.view.photo.author).toBe(first.view.photo.author)
    expect(calls.filter(url => url.includes('open-meteo'))).toHaveLength(1)
    expect(calls.filter(url => url.includes('unsplash'))).toHaveLength(1)
  })

  it('still shows the clock when location and weather are unknown', async () => {
    const { ctx } = runtime()
    globalThis.fetch = async () => new Response('down', { status: 500 })

    const { view } = await (await handleView(request(''), env, ctx, {})).json()
    expect(view.time).toMatch(/^\d{2}:\d{2}$/)
    expect(view.weather).toBeNull()
    expect(view.location).toBeNull()
    expect(view.photo).toBeNull()
  })

  it('uses US units for US visitors only', async () => {
    expect(unitsFor('US')).toMatchObject({ temperature_unit: 'fahrenheit', time_format: '12h' })
    expect(unitsFor('RO')).toEqual({})

    const { ctx } = runtime()
    servicesStub()
    const nyc = { city: 'New York', country: 'US', latitude: '40.7', longitude: '-74.0', timezone: 'America/New_York' }
    const { view } = await (await handleView(request(), env, ctx, nyc)).json()
    expect(view.weather.temperature).toBe('65 °F')
    expect(view.period).toMatch(/^(AM|PM)$/)
  })

  it('computes the visitor\'s wall-clock time', () => {
    const instant = new Date('2026-04-26T07:42:05Z')
    expect(localTime(instant, 'Europe/Bucharest')).toBe('2026-04-26T10:42:05')
    expect(localTime(instant, 'America/New_York')).toBe('2026-04-26T03:42:05')
    expect(localTime(new Date('2026-04-26T00:30:00Z'), 'UTC')).toBe('2026-04-26T00:30:00')
  })

  it('sizes photos for the screen, within bounds', () => {
    const url = new URL(sizedPhotoUrl('https://images.unsplash.com/photo-1?ixid=ABC&w=1080&q=75&fit=max', '99999', 'junk'))
    expect(url.searchParams.get('ixid')).toBe('ABC')
    expect(url.searchParams.get('w')).toBe('3840')
    expect(url.searchParams.get('h')).toBe('1080')
    expect(url.searchParams.getAll('q')).toEqual(['80'])
    expect(url.searchParams.get('fit')).toBe('crop')
  })
})

describe('CORS for the web page', () => {
  const pages = 'https://raduandreigorcea.github.io'

  it('lets the GitHub Pages site read the view and send the attribution ping', async () => {
    const { ctx } = runtime()
    unsplashStub()

    const view = await worker.fetch(new Request('https://proxy.test/api/view', { headers: { Origin: pages } }), env, ctx)
    expect(view.headers.get('Access-Control-Allow-Origin')).toBe(pages)

    const preflight = await worker.fetch(new Request('https://proxy.test/api/photo/download', {
      method: 'OPTIONS', headers: { Origin: pages, 'Access-Control-Request-Method': 'POST' }
    }), env, ctx)
    expect(preflight.status).toBe(204)
    expect(preflight.headers.get('Access-Control-Allow-Origin')).toBe(pages)
    expect(preflight.headers.get('Access-Control-Allow-Headers')).toContain('Content-Type')
  })

  it('gives any other site nothing it can read', async () => {
    const { ctx } = runtime()
    unsplashStub()

    const res = await worker.fetch(new Request('https://proxy.test/api/view', { headers: { Origin: 'https://evil.test' } }), env, ctx)
    expect(res.headers.get('Access-Control-Allow-Origin')).toBeNull()

    const preflight = await worker.fetch(new Request('https://proxy.test/api/photo/download', {
      method: 'OPTIONS', headers: { Origin: 'https://evil.test' }
    }), env, ctx)
    expect(preflight.status).toBe(403)
  })

  it('never opens the app\'s photo endpoint to browsers', async () => {
    const { ctx } = runtime()
    unsplashStub()

    const res = await worker.fetch(new Request('https://proxy.test/api/photo?query=summer', { headers: { Origin: pages } }), env, ctx)
    expect(res.status).toBe(200)
    expect(res.headers.get('Access-Control-Allow-Origin')).toBeNull()
  })

  it('keeps the header on errors, so the page sees a 429 and not a CORS failure', async () => {
    const { ctx } = runtime()
    const limited = { ...env, LIMITER: { limit: async () => ({ success: false }) } }
    const res = await worker.fetch(new Request('https://proxy.test/api/view', { headers: { Origin: pages } }), limited, ctx)
    expect(res.status).toBe(429)
    expect(res.headers.get('Access-Control-Allow-Origin')).toBe(pages)
  })
})
