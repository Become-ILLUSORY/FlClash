//! JNI 层注入的回调函数指针（与 `core.cpp` 的 `JNI_OnLoad` 对应）。
//!
//! 这些符号必须保持和 cgo 自动生成的 `libclash.h` 一致的 C 名称：
//! - `result_func`            InvokeInterface.onResult(String)
//! - `protect_func`           TunInterface.protect(int)
//! - `resolve_process_func`   TunInterface.resolverProcess(int, String, String, int)
//! - `release_object_func`    del_global
//! - `free_string_func`       free
//!
//! 由于 `core.cpp` 会把对应实现直接赋值给这些全局符号，这里必须用
//! `#[no_mangle] pub static mut` 暴露成可写的 C 符号。

use std::os::raw::{c_char, c_int, c_void};

pub type ResultCallback = unsafe extern "C" fn(*mut c_void, *const c_char);
pub type ProtectCallback = unsafe extern "C" fn(*mut c_void, c_int);
pub type ResolveProcessCallback =
    unsafe extern "C" fn(*mut c_void, c_int, *const c_char, *const c_char, c_int) -> *mut c_char;
pub type ReleaseObjectCallback = unsafe extern "C" fn(*mut c_void);
pub type FreeStringCallback = unsafe extern "C" fn(*mut c_char);

#[no_mangle]
pub static mut result_func: Option<ResultCallback> = None;
#[no_mangle]
pub static mut protect_func: Option<ProtectCallback> = None;
#[no_mangle]
pub static mut resolve_process_func: Option<ResolveProcessCallback> = None;
#[no_mangle]
pub static mut release_object_func: Option<ReleaseObjectCallback> = None;
#[no_mangle]
pub static mut free_string_func: Option<FreeStringCallback> = None;

/// 把 result 回调到 JNI 层（InvokeInterface.onResult）。
/// data 是一个 UTF-8 JSON 字符串；调用后不负责释放（Java 侧直接消费）。
pub fn invoke_result(obj: *mut c_void, data: &str) {
    unsafe {
        if let Some(cb) = result_func {
            let s = cstr_from_str(data);
            cb(obj, s);
            drop_cstring(s);
        }
    }
}

/// 调用 protect（Android 把 fd 交给系统 VPN 保护）。
pub fn call_protect(tun_interface: *mut c_void, fd: c_int) {
    unsafe {
        if let Some(cb) = protect_func {
            cb(tun_interface, fd);
        }
    }
}

/// 调用 resolverProcess，返回包名（可为空）。
pub fn call_resolve_process(
    tun_interface: *mut c_void,
    protocol: c_int,
    source: &str,
    target: &str,
    uid: c_int,
) -> Option<String> {
    unsafe {
        let cb = resolve_process_func?;
        let src = cstr_from_str(source);
        let dst = cstr_from_str(target);
        let out = cb(tun_interface, protocol, src, dst, uid);
        drop_cstring(src);
        drop_cstring(dst);
        if out.is_null() {
            return None;
        }
        let s = std::ffi::CStr::from_ptr(out).to_string_lossy().into_owned();
        // resolve 返回值同样由 Java 侧 new_string 消费，这里不 free（与旧实现一致）。
        Some(s)
    }
}

fn cstr_from_str(s: &str) -> *mut c_char {
    std::ffi::CString::new(s).expect("NUL in string").into_raw()
}

fn drop_cstring(p: *mut c_char) {
    unsafe {
        if !p.is_null() {
            let _ = std::ffi::CString::from_raw(p);
        }
    }
}