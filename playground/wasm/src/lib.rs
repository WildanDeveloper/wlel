//! The Wlel front-end as a WebAssembly module.
//!
//! No wasm-bindgen: the ABI is four exported C functions and one shared
//! result buffer, which keeps the module dependency-free and the JavaScript
//! glue about forty lines. See `playground/app.js` for the caller.
//!
//! ```text
//! ptr = wlel_alloc(len)          acquire an input buffer
//! (JS writes the source bytes)
//! n   = wlel_analyze(ptr, len)   analyse; returns the result length
//! out = wlel_result_ptr()        result JSON, n bytes, valid until the next call
//! wlel_dealloc(ptr, len)
//! ```

use std::cell::RefCell;

thread_local! {
    /// The last analysis result. Reused so repeated keystrokes do not grow
    /// the WASM heap.
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Allocate `len` bytes for the caller to fill with UTF-8 source.
#[no_mangle]
pub extern "C" fn wlel_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Free a buffer returned by [`wlel_alloc`].
///
/// # Safety
/// `ptr` must come from [`wlel_alloc`] with the same `len`, and must not be
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn wlel_dealloc(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(Vec::from_raw_parts(ptr, 0, len));
    }
}

/// Analyse a UTF-8 source buffer and return the length of the JSON result.
///
/// # Safety
/// `ptr` must point at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn wlel_analyze(ptr: *const u8, len: usize) -> usize {
    let src = if ptr.is_null() || len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
    };
    let json = std::panic::catch_unwind(|| wlel::playground::analyze_json(src))
        .unwrap_or_else(|_| {
            "{\"ok\":false,\"diagnostics\":[{\"line\":1,\"col\":1,\"endLine\":1,\
             \"endCol\":1,\"severity\":\"error\",\"message\":\"internal error while \
             analysing this buffer\"}],\"formatted\":null}"
                .to_string()
        });
    RESULT.with(|cell| {
        let mut result = cell.borrow_mut();
        result.clear();
        result.extend_from_slice(json.as_bytes());
        result.len()
    })
}

/// Pointer to the JSON from the last [`wlel_analyze`] call.
#[no_mangle]
pub extern "C" fn wlel_result_ptr() -> *const u8 {
    RESULT.with(|cell| cell.borrow().as_ptr())
}

/// The compiler version, as a NUL-terminated static string.
#[no_mangle]
pub extern "C" fn wlel_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}
