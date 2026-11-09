//! World 1.20's supplied dongle response corpus, read through the game view.
//! Missing responses are errors; no keys or game data are compiled in.

use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
};
static LOCAL_BLOCKS: OnceLock<Mutex<HashSet<[u8; 16]>>> = OnceLock::new();
use std::ffi::{CString, c_char, c_void};

unsafe extern "C" fn login(_feature: u32, _vendor: *const c_void, handle: *mut u32) -> u32 {
    if handle.is_null() {
        return 1;
    }
    unsafe {
        *handle = 0x100;
    }
    0
}
unsafe extern "C" fn logout(_handle: u32) -> u32 {
    0
}
unsafe extern "C" fn session_info(
    _handle: u32,
    _format: *const c_char,
    output: *mut *mut c_char,
) -> u32 {
    if output.is_null() {
        return 1;
    }
    let strdup = crate::platform::next(c"strdup");
    if strdup == 0 {
        return 1;
    }
    let strdup: unsafe extern "C" fn(*const c_char) -> *mut c_char =
        unsafe { std::mem::transmute(strdup) };
    unsafe {
        *output = strdup(c"<hasp_info><hasp><haspid>1</haspid></hasp></hasp_info>".as_ptr());
    }
    0
}
unsafe extern "C" fn memory_read(
    _handle: u32,
    _file: u32,
    offset: u32,
    length: u32,
    output: *mut u8,
) -> u32 {
    if output.is_null() {
        return 1;
    }
    let Some(end) = offset.checked_add(length).filter(|&n| n <= 128) else {
        return 1;
    };
    let Some(bytes) = (unsafe { read(c"/game/hasp/hhl_mem.dmp", 128) }) else {
        return 33;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes[offset as usize..end as usize].as_ptr(),
            output,
            length as usize,
        );
    }
    0
}
unsafe extern "C" fn memory_size(_handle: u32, _file: u32, output: *mut u32) -> u32 {
    if output.is_null() {
        return 1;
    }
    unsafe {
        *output = 128;
    }
    0
}
// Preserve the caller so 32-byte asset-key requests cannot be confused with
// the integrity worker's arbitrary 16..1024-byte round-trip buffers.
#[unsafe(naked)]
unsafe extern "C" fn encrypt(_handle: u32, _buffer: *mut u8, _length: u32) -> u32 {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16",
        "push dword ptr [ebp + 4]", "push dword ptr [ebp + 16]",
        "push dword ptr [ebp + 12]", "push dword ptr [ebp + 8]",
        "call {handler}", "mov esp, ebp", "pop ebp", "ret",
        handler = sym encrypt_from,
    );
}
unsafe extern "C" fn encrypt_from(_handle: u32, buffer: *mut u8, length: u32, caller: u32) -> u32 {
    if buffer.is_null() || length == 0 || length > 1024 * 1024 {
        return 1;
    }
    if length == 32 {
        let input = unsafe { std::slice::from_raw_parts(buffer, 32) };
        let key: String = input.iter().map(|byte| format!("{byte:02x}")).collect();
        let name = CString::new(format!("/game/hasp/{key}")).unwrap();
        if let Some(response) = unsafe { read(&name, 32) } {
            unsafe {
                std::ptr::copy_nonoverlapping(response.as_ptr(), buffer, 32);
            }
            return 0;
        }
    }
    if caller == 0x0806c5f3 {
        if length != 32 {
            return 1;
        }
        return unsafe { replay(buffer, length) };
    }
    // Other sites protect live data or round-trip arbitrary graphics buffers.
    let status = unsafe { local_crypt(buffer, length) };
    if caller != 0x081f9bee {
        let encoded = unsafe { std::slice::from_raw_parts(buffer, length as usize) };
        let mut blocks = LOCAL_BLOCKS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for window in encoded.windows(16).step_by(4) {
            blocks.insert(window.try_into().unwrap());
        }
    }
    status
}
#[unsafe(naked)]
unsafe extern "C" fn crypt(_handle: u32, _buffer: *mut u8, _length: u32) -> u32 {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16",
        "push dword ptr [ebp + 4]", "push dword ptr [ebp + 16]",
        "push dword ptr [ebp + 12]", "push dword ptr [ebp + 8]",
        "call {handler}", "mov esp, ebp", "pop ebp", "ret",
        handler = sym decrypt_from,
    );
}
unsafe extern "C" fn decrypt_from(_handle: u32, buffer: *mut u8, length: u32, caller: u32) -> u32 {
    if buffer.is_null() || length == 0 || length > 1024 * 1024 {
        return 1;
    }
    let input = unsafe { std::slice::from_raw_parts(buffer, length as usize) };
    // The game also asks to decode freshly zero-initialized structures.
    if caller != 0x081f9c6c && input.iter().all(|&byte| byte == 0) {
        return 0;
    }
    // These two startup challenges are the only non-local decrypt requests
    // in the fingerprinted build. Require their supplied responses, including
    // exact lengths, rather than treating a missing challenge as live memory.
    if input.starts_with(&[0xdf, 0xa6, 0x42, 0x12, 0xbc, 0x2f, 0x44, 0x16])
        || input.starts_with(&[0x50, 0xd7, 0xf9, 0x74, 0x10, 0x27, 0x7a, 0xc6])
    {
        return unsafe { replay(buffer, length) };
    }
    let produced = input.get(..16).is_some_and(|head| {
        LOCAL_BLOCKS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(<&[u8; 16]>::try_from(head).unwrap())
    });
    if caller == 0x081f9c6c || produced {
        unsafe { local_crypt(buffer, length) }
    } else {
        // Direct game calls may decode a structure before its first encode.
        0
    }
}
unsafe fn local_crypt(buffer: *mut u8, length: u32) -> u32 {
    for byte in unsafe { std::slice::from_raw_parts_mut(buffer, length as usize) } {
        *byte ^= 0xa5;
    }
    0
}
unsafe fn replay(buffer: *mut u8, length: u32) -> u32 {
    let input = unsafe { std::slice::from_raw_parts(buffer, length as usize) };
    let key: String = input.iter().take(32).map(|b| format!("{b:02x}")).collect();
    let name = CString::new(format!("/game/hasp/{key}")).unwrap();
    match unsafe { read(&name, length as usize) } {
        Some(response) => {
            unsafe {
                std::ptr::copy_nonoverlapping(response.as_ptr(), buffer, response.len());
            }
            0
        }
        None => {
            eprintln!("[world] missing HASP response: {key} ({length} bytes)");
            33
        }
    }
}

unsafe fn read(name: &std::ffi::CStr, length: usize) -> Option<Vec<u8>> {
    let open = crate::platform::next(c"fopen");
    let read = crate::platform::next(c"fread");
    let close = crate::platform::next(c"fclose");
    if open == 0 || read == 0 || close == 0 {
        return None;
    }
    let open: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void =
        unsafe { std::mem::transmute(open) };
    let read: unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize =
        unsafe { std::mem::transmute(read) };
    let close: unsafe extern "C" fn(*mut c_void) -> i32 = unsafe { std::mem::transmute(close) };
    let file = unsafe { open(name.as_ptr(), c"rb".as_ptr()) };
    if file.is_null() {
        return None;
    }
    let mut bytes = vec![0; length + 1];
    let n = unsafe { read(bytes.as_mut_ptr().cast(), 1, bytes.len(), file) };
    unsafe {
        close(file);
    }
    if n != length {
        return None;
    }
    bytes.truncate(length);
    Some(bytes)
}

pub unsafe fn install() -> Result<(), String> {
    let hooks = [
        (
            0x0839d3d0usize,
            login as *const () as usize,
            &[0x83, 0xec, 0x14, 0x56, 0x53, 0x8b, 0x74][..],
        ),
        (
            0x0839d470,
            logout as *const () as usize,
            &[0x83, 0xec, 0x14, 0x56, 0x53, 0x8b, 0x74][..],
        ),
        (
            0x0839d55c,
            encrypt as *const () as usize,
            &[0x83, 0xec, 0x14, 0x56, 0x53, 0x8b, 0x74][..],
        ),
        (
            0x0839d648,
            crypt as *const () as usize,
            &[0x83, 0xec, 0x14, 0x56, 0x53, 0x8b, 0x74][..],
        ),
        (
            0x0839df50,
            session_info as *const () as usize,
            &[0x83, 0xec, 0x1c, 0x55, 0x57, 0x56, 0x53][..],
        ),
        (
            0x0839e1c8,
            memory_read as *const () as usize,
            &[0x83, 0xec, 0x18, 0x53, 0x8b, 0x5c, 0x24][..],
        ),
        (
            0x0839e360,
            memory_size as *const () as usize,
            &[0x83, 0xec, 0x18, 0x53, 0x8b, 0x5c, 0x24][..],
        ),
    ];
    for &(address, _, expected) in &hooks {
        if unsafe { std::slice::from_raw_parts(address as *const u8, expected.len()) } != expected {
            return Err(format!("Incompatible World HASP prologue at {address:#x}"));
        }
    }
    for (address, target, _) in hooks {
        let mut patch = [0xb8, 0, 0, 0, 0, 0xff, 0xe0];
        patch[1..5].copy_from_slice(&(target as u32).to_le_bytes());
        unsafe {
            std::ptr::copy_nonoverlapping(patch.as_ptr(), address as *mut u8, patch.len());
        }
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn live_memory_round_trips_preserve_arbitrary_subranges_and_zeroes() {
        for length in [16, 32, 204, 256, 512, 936, 46920] {
            let original: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
            let mut memory = original.clone();
            assert_eq!(
                unsafe {
                    super::encrypt_from(0x100, memory.as_mut_ptr(), length as u32, 0x081f9bee)
                },
                0
            );
            assert_eq!(
                unsafe {
                    super::decrypt_from(0x100, memory.as_mut_ptr(), length as u32, 0x081f9c6c)
                },
                0
            );
            assert_eq!(memory, original);
        }
        // The integrity worker also transforms plaintext it did not encode.
        let mut scratch = [0xa5; 64];
        assert_eq!(
            unsafe { super::decrypt_from(0x100, scratch.as_mut_ptr(), 64, 0x081f9c6c) },
            0
        );
        assert_eq!(scratch, [0; 64]);
        assert_eq!(
            unsafe { super::encrypt_from(0x100, scratch.as_mut_ptr(), 64, 0x081f9bee) },
            0
        );
        assert_eq!(scratch, [0xa5; 64]);
        // Direct game structures can be decoded before their first encode.
        let mut structure = [0x37; 80];
        assert_eq!(
            unsafe { super::decrypt_from(0x100, structure.as_mut_ptr(), 80, 0x080c1503) },
            0
        );
        assert_eq!(structure, [0x37; 80]);
        assert_eq!(
            unsafe { super::encrypt_from(0x100, structure.as_mut_ptr(), 80, 0x080c1573) },
            0
        );
        let mut field = structure[4..36].to_vec();
        assert_eq!(
            unsafe { super::decrypt_from(0x100, field.as_mut_ptr(), 32, 0x080c1503) },
            0
        );
        assert_eq!(field, [0x37; 32]);
        let mut empty = [0; 936];
        assert_eq!(
            unsafe { super::encrypt_from(0x100, empty.as_mut_ptr(), 936, 0x081f9bee) },
            0
        );
        assert_ne!(empty, [0; 936]);
        assert_eq!(
            unsafe { super::decrypt_from(0x100, empty.as_mut_ptr(), 936, 0x081f9c6c) },
            0
        );
        assert_eq!(empty, [0; 936]);
    }
}
