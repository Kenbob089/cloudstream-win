//! C ABI used by the WinUI 3 front-end (P/Invoke).
//!
//!   cs_init(dataDir)        -> 0 on success
//!   cs_call(method, args)   -> heap JSON string: {"ok": <value>} or {"error": "<message>"}
//!   cs_free(ptr)            -> frees any string returned by cs_call
//!
//! All strings are UTF-8, NUL-terminated.

use crate::api::Context;
use crate::http::UreqClient;
use crate::store::Store;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

static CTX: OnceLock<Context> = OnceLock::new();

unsafe fn cstr(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    CStr::from_ptr(p).to_str().ok().map(String::from)
}

/// # Safety
/// `data_dir` must be a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn cs_init(data_dir: *const c_char) -> i32 {
    let Some(dir) = cstr(data_dir) else { return 1 };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let store = Store::open(PathBuf::from(dir).join("library.json"));
        let ctx = Context::new(Arc::new(UreqClient::new()), store);
        CTX.set(ctx).is_ok()
    }));
    match result {
        Ok(true) => 0,
        Ok(false) => 0, // already initialised
        Err(_) => 2,
    }
}

/// # Safety
/// `method` and `args` must be valid NUL-terminated UTF-8 strings. Free the result with `cs_free`.
#[no_mangle]
pub unsafe extern "C" fn cs_call(method: *const c_char, args: *const c_char) -> *mut c_char {
    let out: Value = catch_unwind(AssertUnwindSafe(|| {
        let Some(ctx) = CTX.get() else { return json!({ "error": "Core not initialised." }) };
        let Some(method) = cstr(method) else { return json!({ "error": "Invalid method." }) };
        let args_json = cstr(args).unwrap_or_else(|| "{}".into());
        let args: Value = match serde_json::from_str(&args_json) {
            Ok(v) => v,
            Err(e) => return json!({ "error": format!("Invalid arguments: {e}") }),
        };
        match ctx.call(&method, &args) {
            Ok(v) => json!({ "ok": v }),
            Err(e) => json!({ "error": e }),
        }
    }))
    .unwrap_or_else(|_| json!({ "error": "Internal error." }));

    CString::new(out.to_string()).unwrap_or_default().into_raw()
}

/// # Safety
/// `ptr` must come from `cs_call` and be freed at most once.
#[no_mangle]
pub unsafe extern "C" fn cs_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
