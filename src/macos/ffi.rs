//! Minimal CoreFoundation / IOKit FFI bindings.
//!
//! Only the handful of symbols we actually need are declared, so the tool stays
//! dependency free (no `core-foundation`, `io-kit-sys` or `libc` crates).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void, CStr, CString};

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFIndex = isize;
pub type CFTypeID = usize;

pub type mach_port_t = u32;
pub type kern_return_t = c_int;
pub type io_object_t = mach_port_t;
pub type io_service_t = mach_port_t;
pub type io_connect_t = mach_port_t;

pub const KERN_SUCCESS: kern_return_t = 0;

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
pub const kCFNumberSInt32Type: CFIndex = 3;

/// `kIOMainPortDefault` (and the older `kIOMasterPortDefault`) is documented as
/// `MACH_PORT_NULL`. Passing the literal avoids linking against a symbol whose
/// name changed in macOS 12 and keeps the binary running on older releases.
pub const K_IO_MAIN_PORT_DEFAULT: mach_port_t = 0;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFAllocatorDefault: CFAllocatorRef;

    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    pub fn CFStringGetTypeID() -> CFTypeID;

    pub fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        cstr: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub fn CFStringGetCString(
        s: CFStringRef,
        buffer: *mut c_char,
        size: CFIndex,
        encoding: u32,
    ) -> bool;

    pub fn CFNumberCreate(alloc: CFAllocatorRef, ty: CFIndex, value: *const c_void) -> CFNumberRef;

    pub fn CFDictionaryCreate(
        alloc: CFAllocatorRef,
        keys: *const *const c_void,
        values: *const *const c_void,
        num: CFIndex,
        key_cb: *const c_void,
        value_cb: *const c_void,
    ) -> CFDictionaryRef;
    pub static kCFTypeDictionaryKeyCallBacks: c_void;
    pub static kCFTypeDictionaryValueCallBacks: c_void;

    pub fn CFArrayGetCount(a: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(a: CFArrayRef, idx: CFIndex) -> *const c_void;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    pub fn IOServiceMatching(name: *const c_char) -> CFDictionaryRef;
    pub fn IOServiceGetMatchingService(
        main_port: mach_port_t,
        matching: CFDictionaryRef,
    ) -> io_service_t;
    pub fn IOServiceOpen(
        service: io_service_t,
        owning_task: mach_port_t,
        ty: u32,
        connect: *mut io_connect_t,
    ) -> kern_return_t;
    pub fn IOServiceClose(connect: io_connect_t) -> kern_return_t;
    pub fn IOObjectRelease(object: io_object_t) -> kern_return_t;
    pub fn IOConnectCallStructMethod(
        connection: io_connect_t,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> kern_return_t;
}

extern "C" {
    pub static mach_task_self_: mach_port_t;

    pub fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    pub fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

pub const RTLD_LAZY: c_int = 1;

/// RAII wrapper that releases a CoreFoundation object on drop.
pub struct CFObject(pub CFTypeRef);

impl Drop for CFObject {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

pub fn cfstr(s: &str) -> CFObject {
    let c = CString::new(s).unwrap_or_default();
    unsafe {
        CFObject(CFStringCreateWithCString(
            kCFAllocatorDefault,
            c.as_ptr(),
            kCFStringEncodingUTF8,
        ))
    }
}

/// Converts a `CFStringRef` into an owned Rust `String`.
pub fn cfstring_to_string(s: CFStringRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(s) != CFStringGetTypeID() {
            return None;
        }
        let mut buf = [0i8; 512];
        if CFStringGetCString(
            s,
            buf.as_mut_ptr() as *mut c_char,
            buf.len() as CFIndex,
            kCFStringEncodingUTF8,
        ) {
            CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_str()
                .ok()
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

pub fn cfnumber_i32(v: i32) -> CFObject {
    unsafe {
        CFObject(CFNumberCreate(
            kCFAllocatorDefault,
            kCFNumberSInt32Type,
            &v as *const i32 as *const c_void,
        ))
    }
}

/// Builds a `CFDictionary` from string keys and already-created CF values.
pub fn cfdict(pairs: &[(&str, CFTypeRef)]) -> CFObject {
    let keys: Vec<CFObject> = pairs.iter().map(|(k, _)| cfstr(k)).collect();
    let key_ptrs: Vec<*const c_void> = keys.iter().map(|k| k.0).collect();
    let val_ptrs: Vec<*const c_void> = pairs.iter().map(|(_, v)| *v).collect();
    unsafe {
        CFObject(CFDictionaryCreate(
            kCFAllocatorDefault,
            key_ptrs.as_ptr(),
            val_ptrs.as_ptr(),
            pairs.len() as CFIndex,
            &kCFTypeDictionaryKeyCallBacks as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        ))
    }
}

/// Resolves a symbol from an already loaded framework, used for the private
/// `IOHIDEventSystemClient` API which must not be linked against directly.
pub fn sym(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
    let c = CString::new(name).ok()?;
    let p = unsafe { dlsym(handle, c.as_ptr()) };
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

pub fn dlopen_framework(path: &str) -> Option<*mut c_void> {
    let c = CString::new(path).ok()?;
    let h = unsafe { dlopen(c.as_ptr(), RTLD_LAZY) };
    if h.is_null() {
        None
    } else {
        Some(h)
    }
}
