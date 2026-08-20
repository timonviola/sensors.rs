//! Apple Silicon temperature sensors via the private
//! `IOHIDEventSystemClient` API in IOKit.
//!
//! On M-series Macs the SMC only exposes a handful of thermal keys, while the
//! HID sensor services expose per-cluster CPU/GPU/SoC temperatures. The symbols
//! are private, so they are resolved lazily with `dlsym` and the backend simply
//! yields nothing if they are unavailable.

use std::ffi::c_void;

use super::ffi::*;

const K_HID_PAGE_APPLE_VENDOR: i32 = 0xff00;
const K_HID_USAGE_APPLE_VENDOR_TEMPERATURE_SENSOR: i32 = 0x0005;
const K_IOHID_EVENT_TYPE_TEMPERATURE: i64 = 15;
/// Event fields are `type << 16`.
const K_IOHID_EVENT_FIELD_TEMPERATURE: i32 = (K_IOHID_EVENT_TYPE_TEMPERATURE as i32) << 16;

type ClientCreate = unsafe extern "C" fn(CFAllocatorRef) -> CFTypeRef;
type ClientSetMatching = unsafe extern "C" fn(CFTypeRef, CFDictionaryRef) -> i32;
type ClientCopyServices = unsafe extern "C" fn(CFTypeRef) -> CFArrayRef;
type ServiceCopyProperty = unsafe extern "C" fn(CFTypeRef, CFStringRef) -> CFTypeRef;
type ServiceCopyEvent = unsafe extern "C" fn(CFTypeRef, i64, i32, i64) -> CFTypeRef;
type EventGetFloatValue = unsafe extern "C" fn(CFTypeRef, i32) -> f64;

struct Api {
    create: ClientCreate,
    set_matching: ClientSetMatching,
    copy_services: ClientCopyServices,
    copy_property: ServiceCopyProperty,
    copy_event: ServiceCopyEvent,
    get_float: EventGetFloatValue,
}

impl Api {
    fn load() -> Option<Api> {
        let h = dlopen_framework("/System/Library/Frameworks/IOKit.framework/IOKit")?;
        unsafe {
            Some(Api {
                create: std::mem::transmute::<*mut c_void, ClientCreate>(sym(
                    h,
                    "IOHIDEventSystemClientCreate",
                )?),
                set_matching: std::mem::transmute::<*mut c_void, ClientSetMatching>(sym(
                    h,
                    "IOHIDEventSystemClientSetMatching",
                )?),
                copy_services: std::mem::transmute::<*mut c_void, ClientCopyServices>(sym(
                    h,
                    "IOHIDEventSystemClientCopyServices",
                )?),
                copy_property: std::mem::transmute::<*mut c_void, ServiceCopyProperty>(sym(
                    h,
                    "IOHIDServiceClientCopyProperty",
                )?),
                copy_event: std::mem::transmute::<*mut c_void, ServiceCopyEvent>(sym(
                    h,
                    "IOHIDServiceClientCopyEvent",
                )?),
                get_float: std::mem::transmute::<*mut c_void, EventGetFloatValue>(sym(
                    h,
                    "IOHIDEventGetFloatValue",
                )?),
            })
        }
    }
}

#[derive(Clone, Debug)]
pub struct HidSensor {
    pub name: String,
    pub temperature: f64,
}

/// Reads every Apple HID temperature sensor. Returns an empty vector on Intel
/// Macs or if the private API is not available.
pub fn read_temperatures() -> Vec<HidSensor> {
    let api = match Api::load() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    unsafe {
        let client = (api.create)(kCFAllocatorDefault);
        if client.is_null() {
            return out;
        }
        let client = CFObject(client);

        let page = cfnumber_i32(K_HID_PAGE_APPLE_VENDOR);
        let usage = cfnumber_i32(K_HID_USAGE_APPLE_VENDOR_TEMPERATURE_SENSOR);
        let matching = cfdict(&[("PrimaryUsagePage", page.0), ("PrimaryUsage", usage.0)]);
        (api.set_matching)(client.0, matching.0);

        let services = (api.copy_services)(client.0);
        if services.is_null() {
            return out;
        }
        let services = CFObject(services);

        let product_key = cfstr("Product");
        let count = CFArrayGetCount(services.0);
        for i in 0..count {
            let service = CFArrayGetValueAtIndex(services.0, i);
            if service.is_null() {
                continue;
            }
            let name_ref = (api.copy_property)(service, product_key.0);
            let name = cfstring_to_string(name_ref);
            if !name_ref.is_null() {
                CFRelease(name_ref);
            }
            let name = match name {
                Some(n) => n,
                None => continue,
            };
            let event = (api.copy_event)(service, K_IOHID_EVENT_TYPE_TEMPERATURE, 0, 0);
            if event.is_null() {
                continue;
            }
            let value = (api.get_float)(event, K_IOHID_EVENT_FIELD_TEMPERATURE);
            CFRelease(event);
            if plausible(value) {
                out.push(HidSensor {
                    name,
                    temperature: value,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Inactive sensors report exactly 0.0; anything outside this range is noise.
fn plausible(v: f64) -> bool {
    v.is_finite() && v > 1.0 && v < 150.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_field_encoding() {
        assert_eq!(K_IOHID_EVENT_FIELD_TEMPERATURE, 0x000F_0000);
    }

    #[test]
    fn implausible_values_are_dropped() {
        assert!(!plausible(0.0));
        assert!(!plausible(f64::NAN));
        assert!(!plausible(1000.0));
        assert!(plausible(45.2));
    }
}
