//! Rendering fixes for this sdaemon.exe build under Wine's Direct3D 8.
//!
//! Shader handles: the renderer keeps vertex and pixel shader handles in
//! 16-bit fields (for example `movzwl 0x7301f8` before `SetPixelShader`).
//! Microsoft's D3D8 runtime returns small handles; Wine returns
//! `0xF0000001 + n`. Cut to 16 bits, Wine rejects the pixel shader handles and
//! reads the vertex shader handles as FVF codes, so the car materials on the
//! vehicle select screen and in races draw without their textures. The
//! device's shader methods are wrapped so the game sees small handles:
//! `2n + 1` for vertex shaders (an FVF code never has bit 0 set) and `n + 1`
//! for pixel shaders. Handles that already fit in 16 bits (other D3D8
//! implementations) pass through unchanged.
//!
//! Movie texture clear: when a movie opens (the crowd videos in
//! `shared\movies\avi`), the clear routine at 0x4af3a0 locks a render-target
//! texture, ignores the result and fills `pitch * height` bytes at `pBits`.
//! Wine refuses to lock a default-pool render target, so pitch and pBits are
//! stale stack values: here pitch 1 and a pointer into the movie object
//! itself, which wipes the object's second texture pointer and crashes at
//! 0x4af3b1. Allocating the locals with pitch 0 makes a failed lock clear
//! nothing.
//!
//! Every patch is checked against the expected bytes or pointer first.

use core::{mem::transmute, ptr};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

use super::{FlushInstructionCache, GetCurrentProcess, GetProcAddress, VirtualProtect, bytes_match, log};

#[link(name = "kernel32", kind = "raw-dylib", import_name_type = "undecorated")]
unsafe extern "system" {
    fn GetModuleHandleA(name: *const u8) -> isize;
}

/// sdaemon.exe import slot for d3d8!Direct3DCreate8.
const IAT_DIRECT3D_CREATE8: usize = 0x0062_53dc;

/// Movie texture clear: `sub esp,8; cmp dword [device],0; push esi;
/// mov esi,[esp+0x10]; je`. The first three bytes become `push ecx; push 0`:
/// same eight bytes of locals, with the pitch slot (the lower one) zeroed.
const MOVIE_CLEAR: usize = 0x004a_f3a0;
const MOVIE_CLEAR_BYTES: [u8; 17] = [
    0x83, 0xec, 0x08, 0x83, 0x3d, 0x90, 0x0b, 0x78, 0x04, 0x00, 0x56, 0x8b, 0x74, 0x24, 0x10, 0x74,
    0x21,
];
const MOVIE_CLEAR_PATCH: [u8; 3] = [0x51, 0x6a, 0x00];

/// IDirect3D8::CreateDevice.
const D3D_CREATE_DEVICE: usize = 15;
// IDirect3DDevice8 vtable slots.
const CREATE_VERTEX_SHADER: usize = 75;
const SET_VERTEX_SHADER: usize = 76;
const GET_VERTEX_SHADER: usize = 77;
const DELETE_VERTEX_SHADER: usize = 78;
const GET_VERTEX_SHADER_DECLARATION: usize = 81;
const GET_VERTEX_SHADER_FUNCTION: usize = 82;
const CREATE_PIXEL_SHADER: usize = 87;
const SET_PIXEL_SHADER: usize = 88;
const GET_PIXEL_SHADER: usize = 89;
const DELETE_PIXEL_SHADER: usize = 90;
const GET_PIXEL_SHADER_FUNCTION: usize = 93;
const DEVICE_SLOTS: usize = 97;

/// Wine's first shader handle (VS_HIGHESTFIXEDFXF + 1).
const WINE_HANDLE_BASE: u32 = 0xf000_0001;

static REAL_DIRECT3D_CREATE8: AtomicUsize = AtomicUsize::new(0);
static REAL_CREATE_DEVICE: AtomicUsize = AtomicUsize::new(0);
/// Original IDirect3DDevice8 methods, by vtable slot.
static REAL: [AtomicUsize; DEVICE_SLOTS] = [const { AtomicUsize::new(0) }; DEVICE_SLOTS];
static PATCHED_DEVICE_VTABLE: AtomicUsize = AtomicUsize::new(0);
/// Set once the device returned a handle that needed remapping.
static MAPPED_VS: AtomicBool = AtomicBool::new(false);
static MAPPED_PS: AtomicBool = AtomicBool::new(false);

type Object = *mut *mut usize;
type HandleFn = unsafe extern "system" fn(Object, u32) -> i32;
type GetHandleFn = unsafe extern "system" fn(Object, *mut u32) -> i32;
type DataFn = unsafe extern "system" fn(Object, u32, *mut u8, *mut u32) -> i32;
type CreateVsFn = unsafe extern "system" fn(Object, *const u32, *const u32, *mut u32, u32) -> i32;
type CreatePsFn = unsafe extern "system" fn(Object, *const u32, *mut u32) -> i32;
type CreateDeviceFn = unsafe extern "system" fn(Object, u32, u32, isize, u32, *mut u8, *mut Object) -> i32;
type Direct3DCreate8Fn = unsafe extern "system" fn(u32) -> Object;

pub(super) fn install() {
    patch_movie_clear();
    hook_direct3d();
}

fn patch_movie_clear() {
    if !bytes_match(MOVIE_CLEAR, &MOVIE_CLEAR_BYTES) {
        log(b"arcade-wine-hook: movie clear bytes differ; not patched");
        return;
    }
    if unsafe { write_code(MOVIE_CLEAR, &MOVIE_CLEAR_PATCH) } {
        log(b"arcade-wine-hook: movie texture clear guarded");
    }
}

fn hook_direct3d() {
    let real = unsafe {
        let module = GetModuleHandleA(b"d3d8.dll\0".as_ptr());
        if module == 0 { 0 } else { GetProcAddress(module, b"Direct3DCreate8\0".as_ptr()) as usize }
    };
    let slot = unsafe { ptr::read_volatile(IAT_DIRECT3D_CREATE8 as *const usize) };
    if real == 0 || slot != real {
        log(b"arcade-wine-hook: Direct3DCreate8 import not found; shader handles not remapped");
        return;
    }
    REAL_DIRECT3D_CREATE8.store(real, Relaxed);
    if unsafe { write_pointers(IAT_DIRECT3D_CREATE8 as *mut usize, &[(0, direct3d_create8 as *const () as usize)]) } {
        log(b"arcade-wine-hook: shader handle remapping installed");
    }
}

unsafe extern "system" fn direct3d_create8(sdk: u32) -> Object {
    let real: Direct3DCreate8Fn = unsafe { transmute(REAL_DIRECT3D_CREATE8.load(Relaxed)) };
    let d3d = unsafe { real(sdk) };
    if !d3d.is_null() {
        let vtable = unsafe { *d3d };
        let current = unsafe { *vtable.add(D3D_CREATE_DEVICE) };
        if current != create_device as *const () as usize {
            REAL_CREATE_DEVICE.store(current, Relaxed);
            unsafe { write_pointers(vtable, &[(D3D_CREATE_DEVICE, create_device as *const () as usize)]) };
        }
    }
    d3d
}

unsafe extern "system" fn create_device(
    d3d: Object,
    adapter: u32,
    kind: u32,
    window: isize,
    flags: u32,
    params: *mut u8,
    out: *mut Object,
) -> i32 {
    let real: CreateDeviceFn = unsafe { transmute(REAL_CREATE_DEVICE.load(Relaxed)) };
    let hr = unsafe { real(d3d, adapter, kind, window, flags, params, out) };
    if hr >= 0 && !out.is_null() && unsafe { !(*out).is_null() } {
        unsafe { wrap_device(**out) };
    }
    hr
}

/// Wraps the shader methods in the device vtable (shared by every device).
unsafe fn wrap_device(vtable: *mut usize) {
    if PATCHED_DEVICE_VTABLE.load(Relaxed) == vtable as usize {
        return;
    }
    let wrappers: [(usize, usize); 11] = [
        (CREATE_VERTEX_SHADER, create_vertex_shader as *const () as usize),
        (SET_VERTEX_SHADER, set_vertex_shader as *const () as usize),
        (GET_VERTEX_SHADER, get_vertex_shader as *const () as usize),
        (DELETE_VERTEX_SHADER, delete_vertex_shader as *const () as usize),
        (GET_VERTEX_SHADER_DECLARATION, get_vertex_shader_declaration as *const () as usize),
        (GET_VERTEX_SHADER_FUNCTION, get_vertex_shader_function as *const () as usize),
        (CREATE_PIXEL_SHADER, create_pixel_shader as *const () as usize),
        (SET_PIXEL_SHADER, set_pixel_shader as *const () as usize),
        (GET_PIXEL_SHADER, get_pixel_shader as *const () as usize),
        (DELETE_PIXEL_SHADER, delete_pixel_shader as *const () as usize),
        (GET_PIXEL_SHADER_FUNCTION, get_pixel_shader_function as *const () as usize),
    ];
    for &(slot, _) in &wrappers {
        REAL[slot].store(unsafe { *vtable.add(slot) }, Relaxed);
    }
    if unsafe { write_pointers(vtable, &wrappers) } {
        PATCHED_DEVICE_VTABLE.store(vtable as usize, Relaxed);
        log(b"arcade-wine-hook: Direct3D device shader methods wrapped");
    }
}

// Handle translation. `n` is the index Wine encodes in its handles.

fn vs_to_game(handle: u32) -> u32 {
    let n = handle.wrapping_sub(WINE_HANDLE_BASE);
    if handle >= WINE_HANDLE_BASE && n < 0x7fff {
        MAPPED_VS.store(true, Relaxed);
        (n << 1) | 1
    } else {
        handle
    }
}

fn vs_to_device(handle: u32) -> u32 {
    if MAPPED_VS.load(Relaxed) && handle & 1 == 1 && handle < 0x1_0000 {
        WINE_HANDLE_BASE + (handle >> 1)
    } else {
        handle
    }
}

fn ps_to_game(handle: u32) -> u32 {
    let n = handle.wrapping_sub(WINE_HANDLE_BASE);
    if handle >= WINE_HANDLE_BASE && n < 0xfffe {
        MAPPED_PS.store(true, Relaxed);
        n + 1
    } else {
        handle
    }
}

fn ps_to_device(handle: u32) -> u32 {
    if MAPPED_PS.load(Relaxed) && handle != 0 && handle < 0x1_0000 {
        WINE_HANDLE_BASE + (handle - 1)
    } else {
        handle
    }
}

fn real<T: Copy>(slot: usize) -> T {
    let f = REAL[slot].load(Relaxed);
    unsafe { core::mem::transmute_copy(&f) }
}

unsafe extern "system" fn create_vertex_shader(
    device: Object,
    declaration: *const u32,
    function: *const u32,
    handle: *mut u32,
    usage: u32,
) -> i32 {
    let hr = unsafe { real::<CreateVsFn>(CREATE_VERTEX_SHADER)(device, declaration, function, handle, usage) };
    if hr >= 0 && !handle.is_null() {
        unsafe { *handle = vs_to_game(*handle) };
    }
    hr
}

unsafe extern "system" fn set_vertex_shader(device: Object, handle: u32) -> i32 {
    unsafe { real::<HandleFn>(SET_VERTEX_SHADER)(device, vs_to_device(handle)) }
}

unsafe extern "system" fn get_vertex_shader(device: Object, handle: *mut u32) -> i32 {
    let hr = unsafe { real::<GetHandleFn>(GET_VERTEX_SHADER)(device, handle) };
    if hr >= 0 && !handle.is_null() {
        unsafe { *handle = vs_to_game(*handle) };
    }
    hr
}

unsafe extern "system" fn delete_vertex_shader(device: Object, handle: u32) -> i32 {
    unsafe { real::<HandleFn>(DELETE_VERTEX_SHADER)(device, vs_to_device(handle)) }
}

unsafe extern "system" fn get_vertex_shader_declaration(device: Object, handle: u32, data: *mut u8, size: *mut u32) -> i32 {
    unsafe { real::<DataFn>(GET_VERTEX_SHADER_DECLARATION)(device, vs_to_device(handle), data, size) }
}

unsafe extern "system" fn get_vertex_shader_function(device: Object, handle: u32, data: *mut u8, size: *mut u32) -> i32 {
    unsafe { real::<DataFn>(GET_VERTEX_SHADER_FUNCTION)(device, vs_to_device(handle), data, size) }
}

unsafe extern "system" fn create_pixel_shader(device: Object, function: *const u32, handle: *mut u32) -> i32 {
    let hr = unsafe { real::<CreatePsFn>(CREATE_PIXEL_SHADER)(device, function, handle) };
    if hr >= 0 && !handle.is_null() {
        unsafe { *handle = ps_to_game(*handle) };
    }
    hr
}

unsafe extern "system" fn set_pixel_shader(device: Object, handle: u32) -> i32 {
    unsafe { real::<HandleFn>(SET_PIXEL_SHADER)(device, ps_to_device(handle)) }
}

unsafe extern "system" fn get_pixel_shader(device: Object, handle: *mut u32) -> i32 {
    let hr = unsafe { real::<GetHandleFn>(GET_PIXEL_SHADER)(device, handle) };
    if hr >= 0 && !handle.is_null() {
        unsafe { *handle = ps_to_game(*handle) };
    }
    hr
}

unsafe extern "system" fn delete_pixel_shader(device: Object, handle: u32) -> i32 {
    unsafe { real::<HandleFn>(DELETE_PIXEL_SHADER)(device, ps_to_device(handle)) }
}

unsafe extern "system" fn get_pixel_shader_function(device: Object, handle: u32, data: *mut u8, size: *mut u32) -> i32 {
    unsafe { real::<DataFn>(GET_PIXEL_SHADER_FUNCTION)(device, ps_to_device(handle), data, size) }
}

// Memory patching.

/// Writes `(index, value)` pointers into a table (an import slot or vtable).
unsafe fn write_pointers(table: *mut usize, entries: &[(usize, usize)]) -> bool {
    let lo = entries.iter().map(|e| e.0).min().unwrap_or(0);
    let hi = entries.iter().map(|e| e.0).max().unwrap_or(0);
    let start = unsafe { table.add(lo) } as *mut u8;
    let size = (hi - lo + 1) * core::mem::size_of::<usize>();
    let mut old = 0;
    if unsafe { VirtualProtect(start, size, 0x04, &mut old) } == 0 {
        log(b"arcade-wine-hook: VirtualProtect failed (pointer table)");
        return false;
    }
    for &(index, value) in entries {
        unsafe { ptr::write_volatile(table.add(index), value) };
    }
    unsafe { VirtualProtect(start, size, old, &mut old) };
    true
}

unsafe fn write_code(addr: usize, bytes: &[u8]) -> bool {
    let mut old = 0;
    if unsafe { VirtualProtect(addr as *mut u8, bytes.len(), 0x40, &mut old) } == 0 {
        log(b"arcade-wine-hook: VirtualProtect failed (code)");
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        unsafe { ptr::write_volatile((addr + i) as *mut u8, *b) };
    }
    unsafe {
        VirtualProtect(addr as *mut u8, bytes.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), addr as *const u8, bytes.len());
    }
    true
}
