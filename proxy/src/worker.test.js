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
