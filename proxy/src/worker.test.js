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
