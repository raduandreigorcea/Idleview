// Draws a view (idleview_core::View). Everything arrives already formatted, so this
// only places it: no fetching, no formatting, no schedules. The app and the web page
// both use this file.

const byId = (id) => document.getElementById(id);
const one = (selector) => document.querySelector(selector);

function setText(el, text) {
    if (el && el.textContent !== text) el.textContent = text;
}

function show(el, visible) {
    if (el) el.hidden = !visible;
}

// Unsplash names and profile URLs are user-controlled, so links are built as nodes and
// every URL is scheme-checked (a `javascript:` href would run on click).
function safeHttpUrl(value) {
    try {
        const url = new URL(value);
        return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
    } catch (e) {
        return null;
    }
}

// Photos also come from disk: the user's own, through Tauri's asset protocol
// (asset://localhost/... on macOS/Linux, http://asset.localhost/... on Windows).
function safeImageUrl(value) {
    try {
        return new URL(value).protocol === 'asset:' ? value : safeHttpUrl(value);
    } catch (e) {
        return null;
    }
}

function link(href, text) {
    const safe = safeHttpUrl(href);
    const el = document.createElement(safe ? 'a' : 'span');
    if (safe) {
        el.href = safe;
        el.target = '_blank';
        el.rel = 'noopener noreferrer';
    }
    el.textContent = text;
    return el;
}

let shownPhotoUrl = null;
let creditTimer = null;

// Swap the background only once the new photo has loaded, so it never flashes blank.
function showPhoto(photo) {
    const url = safeImageUrl(photo.url);
    if (!url) return;
    shownPhotoUrl = photo.url;

    const img = new Image();
    img.onload = img.onerror = () => {
        if (shownPhotoUrl !== photo.url) return; // a newer photo arrived meanwhile
        document.body.style.backgroundImage = `url("${url.replace(/"/g, '%22')}")`;

        // The user's own photos carry no author, and need no credit.
        const credit = byId('photo-credit');
        clearTimeout(creditTimer);
        if (!photo.author) {
            credit.classList.add('hidden');
            return;
        }
        credit.replaceChildren(
            'Photo by ',
            link(photo.author_url, photo.author || 'Unknown'),
            ' on ',
            link('https://unsplash.com', 'Unsplash')
        );
        credit.classList.remove('hidden');
        creditTimer = setTimeout(() => credit.classList.add('hidden'), 10000);
    };
    img.src = url;
}

export function render(view) {
    const s = view.show;

    setText(byId('time-value'), view.time);
    setText(byId('time-period'), view.period || '');
    show(byId('time-period'), !!view.period);
    show(byId('time'), s.show_clock);

    setText(byId('weekday'), view.weekday);
    setText(byId('date-value'), view.date);
    show(byId('weekday'), s.show_weekday);
    show(byId('date-value'), s.show_date);
    show(byId('date'), s.show_weekday || s.show_date);

    setText(byId('location'), view.location || 'Locating…');
    show(one('.location-badge'), s.show_location);

    const metrics = {
        sunrise: s.show_sunrise_sunset,
        sunset: s.show_sunrise_sunset,
        precipitation: s.show_precipitation_cloudiness,
        cloudiness: s.show_precipitation_cloudiness,
        humidity: s.show_humidity_wind,
        wind: s.show_humidity_wind,
    };
    const anyMetric = Object.keys(metrics).some((key) => metrics[key]);
    Object.keys(metrics).forEach((key) => show(one(`[data-metric="${key}"]`), metrics[key]));
    show(one('.metrics-grid'), anyMetric);
    show(one('.main-weather-status'), s.show_temperature);
    one('.main-weather-status').classList.toggle('no-metrics', !anyMetric);
    show(one('.bottom-section'), s.show_temperature || anyMetric);

    const w = view.weather;
    if (w) {
        setText(byId('temp'), w.temperature);
        setText(byId('sunrise'), w.sunrise);
        setText(byId('sunset'), w.sunset);
        setText(byId('precipitation'), w.precip_value);
        setText(byId('humidity'), w.humidity);
        setText(byId('wind'), w.wind);
        setText(byId('cloudiness'), w.clouds);

        const tile = one('[data-metric="precipitation"]');
        const icon = tile.querySelector('.metric-icon');
        const src = `assets/${w.precip_icon}`;
        if (icon.getAttribute('src') !== src) {
            icon.src = src;
            icon.alt = w.precip_label;
        }
        setText(tile.querySelector('.metric-label'), w.precip_label);
    }

    if (view.photo && view.photo.url !== shownPhotoUrl) {
        showPhoto(view.photo);
    } else if (!view.photo && shownPhotoUrl) {
        // "My photos" with an empty library: plain dark background, no stale photo.
        shownPhotoUrl = null;
        document.body.style.backgroundImage = '';
        byId('photo-credit').classList.add('hidden');
    }
}
