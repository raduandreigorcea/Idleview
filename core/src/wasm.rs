//! The core, callable from the photo Worker's JavaScript.
//!
//! A deliberately tiny ABI instead of wasm-bindgen: JSON in, JSON out, through linear
//! memory. The Worker writes the request with `alloc`, calls a function, and reads the
//! answer back; the loader is proxy/src/core.js. No extra toolchain beyond the
//! wasm32 target.

use chrono::NaiveDateTime;
use serde::Deserialize;

use crate::{Photo, Units, Visibility, Weather};

#[derive(Deserialize)]
struct Request {
    /// Local time where the viewer is, e.g. "2026-04-26T10:42:00".
    now: NaiveDateTime,
    #[serde(default)]
    units: Units,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    weather: Option<Weather>,
    #[serde(default)]
    photo: Option<Photo>,
}

/// Reserve `len` bytes for the caller to write a request into.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Free memory handed out by `alloc`, or returned by one of the functions below.
///
/// # Safety
/// `ptr` and `len` must describe a block this module allocated.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len));
}

/// The photo search for this viewer's time and weather. Festive searches are always on:
/// the web page has no settings.
///
/// # Safety
/// `ptr`/`len` must be a request written into memory from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn photo_query(ptr: *mut u8, len: usize) -> u64 {
    respond(ptr, len, |request| {
        serde_json::to_string(&crate::photo_query(request.now, request.weather.as_ref(), true))
    })
}

/// The finished view, the same shape the desktop app emits.
///
/// # Safety
/// `ptr`/`len` must be a request written into memory from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn view(ptr: *mut u8, len: usize) -> u64 {
    respond(ptr, len, |request| {
        let mut units = request.units.clone();
        units.validate();
        serde_json::to_string(&crate::view(
            request.now,
            &units,
            &Visibility::default(),
            request.location.as_deref(),
            request.weather.as_ref(),
            request.photo.as_ref(),
        ))
    })
}

/// Parse the request, run `f`, and return the JSON answer as `(ptr << 32) | len`.
/// A bad request answers `{"error": "..."}` rather than trapping.
unsafe fn respond(
    ptr: *mut u8,
    len: usize,
    f: impl FnOnce(&Request) -> serde_json::Result<String>,
) -> u64 {
    let input = Vec::from_raw_parts(ptr, len, len);
    let output = serde_json::from_slice::<Request>(&input)
        .and_then(|request| f(&request))
        .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }).to_string());

    let mut bytes = output.into_bytes().into_boxed_slice();
    let (out_ptr, out_len) = (bytes.as_mut_ptr(), bytes.len());
    std::mem::forget(bytes);
    ((out_ptr as u64) << 32) | out_len as u64
}
