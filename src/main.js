// The app's side of the dashboard. Rust (src-tauri/src/dashboard.rs) decides what the
// screen shows and emits it as `view`; this draws it and shows the pairing card.

import { render } from './render.js';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Listen first, then ask for the current view, so nothing emitted in between is lost.
listen('view', (event) => render(event.payload))
    .then(() => invoke('get_view'))
    .then((view) => view && render(view));

// ===== Pairing =====
//
// The control panel needs a token to change anything, and this screen is the only
// place it is shown: for 30 seconds at startup, so a phone can be paired even on a
// screen with no keyboard, and on demand with T.

let pairingTimer = null;

async function showPairingCard(durationMs) {
    let info;
    try {
        info = await invoke('get_server_info');
    } catch (error) {
        console.error('Failed to read server info:', error);
        return;
    }

    let card = document.getElementById('pairing-card');
    if (!card) {
        card = document.createElement('div');
        card.id = 'pairing-card';
        document.body.appendChild(card);
    }

    const line = (className, text) => {
        const div = document.createElement('div');
        div.className = className;
        div.textContent = text;
        return div;
    };

    // The LAN address: 127.0.0.1 is useless to the phone being paired.
    const lanUrl = info.urls.find((url) => !url.includes('127.0.0.1')) || info.urls[0];

    card.replaceChildren(
        line('pairing-title', 'Control panel'),
        line('pairing-url', lanUrl),
        line('pairing-label', 'Token'),
        line('pairing-token', info.token),
        line('pairing-hint', 'Press T to show or hide')
    );
    card.classList.remove('hidden');

    clearTimeout(pairingTimer);
    if (durationMs > 0) pairingTimer = setTimeout(() => card.classList.add('hidden'), durationMs);
}

function togglePairingCard() {
    const card = document.getElementById('pairing-card');
    if (!card || card.classList.contains('hidden')) {
        showPairingCard(0);
    } else {
        clearTimeout(pairingTimer);
        card.classList.add('hidden');
    }
}

document.addEventListener('contextmenu', (event) => event.preventDefault());
document.addEventListener('keydown', (event) => {
    if (event.key === 't' || event.key === 'T') togglePairingCard();
});

showPairingCard(30000);
