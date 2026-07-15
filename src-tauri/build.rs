fn main() {
    // The Unsplash key is deliberately NOT embedded here any more.
    //
    // This script used to read .env and hand the key to rustc via `cargo:rustc-env`,
    // which `option_env!` then resolved into a string literal baked into the binary -
    // recoverable from any shipped installer with a plain `grep`. The app now holds no
    // key at all: photos are fetched through the proxy in `proxy/`, which keeps the key
    // server-side (see src/photos.rs).
    tauri_build::build()
}
