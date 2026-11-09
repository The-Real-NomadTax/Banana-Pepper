//! Movie playback under Wine.
//!
//! The game plays its WMV movies through a Media Foundation source reader and
//! asks for RGB32 frames. On Windows the WMV decoder outputs RGB32 itself;
//! Wine's source decodes to YUV only and converts to RGB32 only when the
//! reader has video processing enabled. Without it `SetCurrentMediaType`
//! fails (MF_E_TOPO_CODEC_NOT_FOUND) and the game stops on an assertion in
//! attract mode. The reader the game creates gets
//! `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`; Windows ignores it when the
//! decoder already outputs the asked type.
use core::ffi::c_void;

/// MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING
/// {fb394f3d-ccf1-42ee-bbb3-f9b845d5681d}.
const ENABLE_VIDEO_PROCESSING: [u8; 16] = [
    0x3d, 0x4f, 0x39, 0xfb, 0xf1, 0xcc, 0xee, 0x42, 0xbb, 0xb3, 0xf9, 0xb8, 0x45, 0xd5, 0x68, 0x1d,
];

#[link(name = "mfplat", kind = "raw-dylib")]
unsafe extern "system" {
    fn MFCreateAttributes(attributes: *mut *mut c_void, size: u32) -> i32;
}

/// `IMFAttributes::SetUINT32` (vtable slot 21) and `Release` (slot 2).
type SetUint32 = unsafe extern "system" fn(*mut c_void, *const u8, u32) -> i32;
type Release = unsafe extern "system" fn(*mut c_void) -> u32;

fn vtable(object: *mut c_void, slot: usize) -> usize {
    unsafe { *(*(object as *const *const usize)).add(slot) }
}

/// The attributes to create a reader with: the game's, or new ones, with
/// video processing enabled. Returns whether new ones were created (to be
/// released after the call).
fn with_video_processing(attributes: *mut c_void) -> (*mut c_void, bool) {
    let (attributes, created) = if attributes.is_null() {
        let mut new = core::ptr::null_mut();
        if unsafe { MFCreateAttributes(&mut new, 1) } < 0 || new.is_null() {
            return (attributes, false);
        }
        (new, true)
    } else {
        (attributes, false)
    };
    let set: SetUint32 = unsafe { core::mem::transmute(vtable(attributes, 21)) };
    unsafe { set(attributes, ENABLE_VIDEO_PROCESSING.as_ptr(), 1) };
    (attributes, created)
}

fn release(attributes: *mut c_void) {
    let release: Release = unsafe { core::mem::transmute(vtable(attributes, 2)) };
    unsafe { release(attributes) };
}

/// `MFCreateSourceReaderFromURL`.
pub unsafe extern "system" fn create_source_reader_from_url(
    url: *const u16,
    attributes: *mut c_void,
    reader: *mut *mut c_void,
) -> i32 {
    let f: unsafe extern "system" fn(*const u16, *mut c_void, *mut *mut c_void) -> i32 =
        unsafe { core::mem::transmute(super::original(b"MFCreateSourceReaderFromURL")) };
    let (attributes, created) = with_video_processing(attributes);
    let result = unsafe { f(url, attributes, reader) };
    if created {
        release(attributes);
    }
    result
}

/// `MFCreateSourceReaderFromMediaSource`.
pub unsafe extern "system" fn create_source_reader_from_media_source(
    source: *mut c_void,
    attributes: *mut c_void,
    reader: *mut *mut c_void,
) -> i32 {
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> i32 =
        unsafe { core::mem::transmute(super::original(b"MFCreateSourceReaderFromMediaSource")) };
    let (attributes, created) = with_video_processing(attributes);
    let result = unsafe { f(source, attributes, reader) };
    if created {
        release(attributes);
    }
    result
}
