// The browser's side of the screen. The Worker that serves this page also computes the
// view (GET /api/view) from the visitor's location and local time, using the same Rust
// core as the desktop app; this fetches it on the minute and draws it. No settings.

import { render } from './render.js';

let shownPhoto = null;

async function update() {
    // Physical pixels, so a HiDPI screen gets a sharp photo.
    const scale = window.devicePixelRatio || 1;
    const width = Math.round(window.innerWidth * scale);
    const height = Math.round(window.innerHeight * scale);

    try {
        const response = await fetch(`/api/view?w=${width}&h=${height}`);
        if (!response.ok) return;
        const { view, download_location: downloadLocation } = await response.json();
        render(view);

        // Unsplash's terms: ping its download endpoint once each photo is shown.
        if (view.photo && view.photo.url !== shownPhoto) {
            shownPhoto = view.photo.url;
            if (downloadLocation) {
                fetch('/api/photo/download', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ downloadUrl: downloadLocation }),
                }).catch(() => {});
            }
        }
    } catch (error) {
        // Offline for a moment: keep showing what is there, try again next minute.
    }
}

// Just past each minute boundary, so the clock flips on time.
function scheduleNext() {
    const now = new Date();
    const wait = (60 - now.getSeconds()) * 1000 - now.getMilliseconds() + 250;
    setTimeout(() => update().then(scheduleNext), wait);
}

update().then(scheduleNext);
