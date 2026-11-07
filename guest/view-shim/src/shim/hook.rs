//! Minimal x86 inline hooks: copy whole prologue instructions (at least five
//! bytes) into a trampoline that jumps back, then replace them with a jump to
//! the detour. Prologues containing relative control flow are refused.
#![allow(unsafe_op_in_unsafe_fn)]
use iced_x86::{Decoder, DecoderOptions, FlowControl};
use std::ptr;
use windows_sys::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc, VirtualProtect},
    Threading::GetCurrentProcess,
};

/// Redirect `target` to `detour`; returns a callable pointer to the original.
pub unsafe fn install(target: usize, detour: usize) -> Result<usize, String> {
    let code = std::slice::from_raw_parts(target as *const u8, 32);
    let mut decoder = Decoder::with_ip(32, code, target as u64, DecoderOptions::NONE);
    let mut len = 0usize;
    while len < 5 {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err(format!("undecodable prologue at {target:#x}"));
        }
        if instruction.flow_control() != FlowControl::Next {
            return Err(format!("cannot relocate {:?} at {target:#x}", instruction.code()));
        }
        len += instruction.len();
    }
    let trampoline = VirtualAlloc(
        ptr::null(),
        64,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    ) as *mut u8;
    if trampoline.is_null() {
        return Err("cannot allocate a trampoline".into());
    }
    ptr::copy_nonoverlapping(target as *const u8, trampoline, len);
    jump(trampoline.add(len), target + len);
    let mut old = 0;
    if VirtualProtect(target as *const _, len, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return Err(format!("cannot unprotect {target:#x}"));
    }
    jump(target as *mut u8, detour);
    for i in 5..len {
        *(target as *mut u8).add(i) = 0x90;
    }
    VirtualProtect(target as *const _, len, old, &mut old);
    FlushInstructionCache(GetCurrentProcess(), target as *const _, len);
    Ok(trampoline as usize)
}

unsafe fn jump(at: *mut u8, to: usize) {
    *at = 0xe9;
    let relative = (to as isize - (at as isize + 5)) as i32;
    ptr::write_unaligned(at.add(1) as *mut i32, relative);
}
