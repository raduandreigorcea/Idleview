// The same page runs in two places: inside the desktop app, where Rust sends the view,
// and in a browser, where the photo Worker does (see web.js).
import(window.__TAURI__ ? './tauri.js' : './web.js');
