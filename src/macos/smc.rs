//! Apple SMC (System Management Controller) access through the `AppleSMC`
//! IOKit user client.
//!
//! Works on both Intel and Apple Silicon Macs and needs no elevated privileges.
//! Provides fan speeds, power/voltage/current rails and (mostly on Intel) the
//! CPU/GPU temperature keys.

use std::ffi::{c_void, CString};

use super::ffi::*;

const KERNEL_INDEX_SMC: u32 = 2;
const SMC_CMD_READ_BYTES: u8 = 5;
const SMC_CMD_READ_INDEX: u8 = 8;
const SMC_CMD_READ_KEYINFO: u8 = 9;

/// Mirrors `SMCKeyData_t` from Apple's SMC user client (80 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SMCKeyData {
    key: u32,
    vers: [u8; 6],
    _pad0: [u8; 2],
    p_limit: [u8; 16],
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
    _pad1: [u8; 3],
    result: u8,
    status: u8,
    data8: u8,
    _pad2: u8,
    data32: u32,
    bytes: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyInfo {
    pub size: u32,
    pub data_type: [u8; 4],
}

pub struct Smc {
    conn: io_connect_t,
}

impl Smc {
    pub fn open() -> Option<Smc> {
        unsafe {
            let name = CString::new("AppleSMC").ok()?;
            let matching = IOServiceMatching(name.as_ptr());
            if matching.is_null() {
                return None;
            }
            // IOServiceGetMatchingService consumes the matching dictionary.
            let service = IOServiceGetMatchingService(K_IO_MAIN_PORT_DEFAULT, matching);
            if service == 0 {
                return None;
            }
            let mut conn: io_connect_t = 0;
            let rc = IOServiceOpen(service, mach_task_self_, 0, &mut conn);
            IOObjectRelease(service);
            if rc != KERN_SUCCESS || conn == 0 {
                return None;
            }
            Some(Smc { conn })
        }
    }

    fn call(&self, input: &SMCKeyData) -> Option<SMCKeyData> {
        let mut output = SMCKeyData::default();
        let mut out_size = std::mem::size_of::<SMCKeyData>();
        let rc = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                KERNEL_INDEX_SMC,
                input as *const SMCKeyData as *const c_void,
                std::mem::size_of::<SMCKeyData>(),
                &mut output as *mut SMCKeyData as *mut c_void,
                &mut out_size,
            )
        };
        if rc != KERN_SUCCESS || output.result != 0 {
            return None;
        }
        Some(output)
    }

    /// Number of keys exposed by the SMC (the `#KEY` pseudo key).
    pub fn key_count(&self) -> Option<u32> {
        let (info, data) = self.read_raw("#KEY")?;
        if info.size < 4 {
            return None;
        }
        Some(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
    }

    /// Resolves the key stored at `index` in the SMC key table.
    pub fn key_at(&self, index: u32) -> Option<String> {
        let input = SMCKeyData {
            data8: SMC_CMD_READ_INDEX,
            data32: index,
            ..SMCKeyData::default()
        };
        let out = self.call(&input)?;
        Some(key_to_string(out.key))
    }

    pub fn key_info(&self, key: &str) -> Option<KeyInfo> {
        let input = SMCKeyData {
            key: string_to_key(key)?,
            data8: SMC_CMD_READ_KEYINFO,
            ..SMCKeyData::default()
        };
        let out = self.call(&input)?;
        Some(KeyInfo {
            size: out.data_size,
            data_type: out.data_type.to_be_bytes(),
        })
    }

    fn read_raw(&self, key: &str) -> Option<(KeyInfo, [u8; 32])> {
        let info = self.key_info(key)?;
        if info.size == 0 || info.size as usize > 32 {
            return None;
        }
        let input = SMCKeyData {
            key: string_to_key(key)?,
            data8: SMC_CMD_READ_BYTES,
            data_size: info.size,
            data_type: u32::from_be_bytes(info.data_type),
            ..SMCKeyData::default()
        };
        let out = self.call(&input)?;
        Some((info, out.bytes))
    }

    /// Reads a key and decodes it to an `f64` according to its SMC data type.
    pub fn read(&self, key: &str) -> Option<f64> {
        let (info, bytes) = self.read_raw(key)?;
        decode(&info, &bytes[..info.size as usize])
    }

    /// Lists every key the SMC exposes, in table order.
    pub fn keys(&self) -> Vec<String> {
        let count = self.key_count().unwrap_or(0);
        (0..count).filter_map(|i| self.key_at(i)).collect()
    }
}

impl Drop for Smc {
    fn drop(&mut self) {
        unsafe {
            IOServiceClose(self.conn);
        }
    }
}

fn string_to_key(key: &str) -> Option<u32> {
    let b = key.as_bytes();
    if b.len() != 4 {
        return None;
    }
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn key_to_string(key: u32) -> String {
    String::from_utf8_lossy(&key.to_be_bytes()).to_string()
}

/// Decodes an SMC value.
///
/// Handles IEEE floats (`flt `), Apple's fixed point types (`sp78`, `fpe2`,
/// `fp88`, ...) and plain integers (`ui8 `, `ui16`, `ui32`, `si8 `, `si16`).
pub fn decode(info: &KeyInfo, data: &[u8]) -> Option<f64> {
    let ty = std::str::from_utf8(&info.data_type).ok()?;
    let ty = ty.trim_end_matches([' ', '\0']);
    match ty {
        "flt" => {
            if data.len() < 4 {
                return None;
            }
            Some(f32::from_le_bytes([data[0], data[1], data[2], data[3]]) as f64)
        }
        "ui8" | "ui16" | "ui32" | "ui64" => Some(be_uint(data) as f64),
        "si8" | "si16" | "si32" => Some(be_int(data) as f64),
        "hex" | "ch8*" => None,
        _ => {
            // Fixed point: [sf]pXY where X = integer bits, Y = fraction bits.
            let b = ty.as_bytes();
            if b.len() == 4 && (b[0] == b's' || b[0] == b'f') && b[1] == b'p' {
                let frac = (b[3] as char).to_digit(16)?;
                let signed = b[0] == b's';
                let raw = if signed {
                    be_int(data) as f64
                } else {
                    be_uint(data) as f64
                };
                return Some(raw / (1u64 << frac) as f64);
            }
            None
        }
    }
}

fn be_uint(data: &[u8]) -> u64 {
    data.iter()
        .take(8)
        .fold(0u64, |acc, b| (acc << 8) | *b as u64)
}

fn be_int(data: &[u8]) -> i64 {
    let bits = (data.len().min(8) * 8) as u32;
    let raw = be_uint(data);
    if bits < 64 && raw & (1 << (bits - 1)) != 0 {
        raw as i64 - (1i64 << bits)
    } else {
        raw as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(ty: &str, size: u32) -> KeyInfo {
        let mut t = [b' '; 4];
        for (i, c) in ty.bytes().take(4).enumerate() {
            t[i] = c;
        }
        KeyInfo { size, data_type: t }
    }

    /// Offsets verified against Apple's `SMCKeyData_t` compiled with a C
    /// compiler; a mismatch would silently corrupt every SMC call.
    #[test]
    fn struct_layout_matches_apple() {
        assert_eq!(std::mem::size_of::<SMCKeyData>(), 80);
        let v = SMCKeyData::default();
        let base = &v as *const _ as usize;
        macro_rules! off {
            ($f:ident) => {
                (&v.$f as *const _ as usize) - base
            };
        }
        assert_eq!(off!(key), 0);
        assert_eq!(off!(vers), 4);
        assert_eq!(off!(p_limit), 12);
        assert_eq!(off!(data_size), 28);
        assert_eq!(off!(data_type), 32);
        assert_eq!(off!(data_attributes), 36);
        assert_eq!(off!(result), 40);
        assert_eq!(off!(status), 41);
        assert_eq!(off!(data8), 42);
        assert_eq!(off!(data32), 44);
        assert_eq!(off!(bytes), 48);
    }

    #[test]
    fn key_roundtrip() {
        assert_eq!(string_to_key("TC0P"), Some(0x5443_3050));
        assert_eq!(key_to_string(0x5443_3050), "TC0P");
        assert_eq!(string_to_key("TOOLONG"), None);
    }

    #[test]
    fn decodes_sp78_temperature() {
        // 45.5 C as sp78 = 45.5 * 256 = 11648 = 0x2D80
        assert_eq!(decode(&info("sp78", 2), &[0x2D, 0x80]), Some(45.5));
    }

    #[test]
    fn decodes_negative_sp78() {
        assert_eq!(decode(&info("sp78", 2), &[0xFF, 0x00]), Some(-1.0));
    }

    #[test]
    fn decodes_fpe2_fan_rpm() {
        // fpe2: 2 fractional bits -> 1234 RPM = 4936 = 0x1348
        assert_eq!(decode(&info("fpe2", 2), &[0x13, 0x48]), Some(1234.0));
    }

    #[test]
    fn decodes_float_little_endian() {
        let bytes = 42.5f32.to_le_bytes();
        assert_eq!(decode(&info("flt ", 4), &bytes), Some(42.5));
    }

    #[test]
    fn decodes_unsigned_ints() {
        assert_eq!(decode(&info("ui8 ", 1), &[0x2A]), Some(42.0));
        assert_eq!(decode(&info("ui16", 2), &[0x01, 0x00]), Some(256.0));
        assert_eq!(decode(&info("ui32", 4), &[0, 0, 0x04, 0xD2]), Some(1234.0));
    }

    #[test]
    fn rejects_unknown_types() {
        assert_eq!(decode(&info("ch8*", 4), &[0, 0, 0, 0]), None);
    }
}
