//! Big Buck Hunter crosshair (settings.cfg `crosshair`, via ARCADE_CROSSHAIR).
//!
//! The game's own `glutSwapBuffers` call is interposed: the crosshair is
//! drawn over the finished frame at the current aim position (mouse or
//! controller), then the real swap runs. Every piece of GL state touched is
//! saved and restored, and the game is fixed-function GL 1.x only.
use crate::crosshair;
use std::ffi::{c_int, c_void};
use std::sync::OnceLock;

#[link(name = "GL")]
unsafe extern "C" {
    fn glPushAttrib(mask: u32);
    fn glPopAttrib();
    fn glPushClientAttrib(mask: u32);
    fn glPopClientAttrib();
    fn glMatrixMode(mode: u32);
    fn glPushMatrix();
    fn glPopMatrix();
    fn glLoadIdentity();
    fn glOrtho(l: f64, r: f64, b: f64, t: f64, n: f64, f: f64);
    fn glViewport(x: i32, y: i32, w: i32, h: i32);
    fn glEnable(cap: u32);
    fn glDisable(cap: u32);
    fn glBlendFunc(s: u32, d: u32);
    fn glColorMask(r: u8, g: u8, b: u8, a: u8);
    fn glPolygonMode(face: u32, mode: u32);
    fn glGenTextures(n: i32, textures: *mut u32);
    fn glBindTexture(target: u32, texture: u32);
    fn glTexImage2D(
        target: u32,
        level: i32,
        internal: i32,
        w: i32,
        h: i32,
        border: i32,
        format: u32,
        kind: u32,
        data: *const c_void,
    );
    fn glTexParameteri(target: u32, name: u32, value: i32);
    fn glTexEnvi(target: u32, name: u32, value: i32);
    fn glPixelStorei(name: u32, value: i32);
    fn glColor4f(r: f32, g: f32, b: f32, a: f32);
    fn glBegin(mode: u32);
    fn glEnd();
    fn glTexCoord2f(s: f32, t: f32);
    fn glVertex2f(x: f32, y: f32);
}

const ALL_ATTRIB_BITS: u32 = 0x000f_ffff;
const CLIENT_PIXEL_STORE_BIT: u32 = 1;
const MODELVIEW: u32 = 0x1700;
const PROJECTION: u32 = 0x1701;
const TEXTURE: u32 = 0x1702;
const TEXTURE_2D: u32 = 0x0de1;
const DISABLED: [u32; 11] = [
    0x0b71, // DEPTH_TEST
    0x0b50, // LIGHTING
    0x0b44, // CULL_FACE
    0x0b60, // FOG
    0x0bc0, // ALPHA_TEST
    0x0b90, // STENCIL_TEST
    0x0c11, // SCISSOR_TEST
    0x0b57, // COLOR_MATERIAL
    0x0c60, // TEXTURE_GEN_S
    0x0c61, // TEXTURE_GEN_T
    0x0de0, // TEXTURE_1D
];
const BLEND: u32 = 0x0be2;
/// Textures are padded to a power of two for old GL 1.x drivers.
const SIDE: usize = 128;

/// Per player: (texture, image width, image height), created on first use in
/// the game's GL context. `None` when the crosshair is off or the image is
/// unusable.
static TEXTURE_ID: [OnceLock<Option<(u32, u32, u32)>>; 2] = [OnceLock::new(), OnceLock::new()];

pub fn enabled() -> bool {
    style().is_some()
}

fn style() -> Option<&'static [u8]> {
    player_style(0)
}

/// Player one's configured style; player two gets the other colour.
fn player_style(player: usize) -> Option<&'static [u8]> {
    let id = std::env::var("ARCADE_CROSSHAIR").ok()?;
    crosshair::png(&id)?;
    crosshair::png(if player == 0 { &id } else { crosshair::player_two(&id) })
}

/// RGBA8 pixels, padded into a SIDE x SIDE square.
pub fn decode(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("crosshair too large")?];
    let info = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "crosshair is {:?} {:?}, not RGBA8",
            info.color_type, info.bit_depth
        ));
    }
    let (w, h) = (info.width as usize, info.height as usize);
    if w > SIDE || h > SIDE {
        return Err(format!("crosshair is {w}x{h}; at most {SIDE}x{SIDE}"));
    }
    let mut square = vec![0; SIDE * SIDE * 4];
    for row in 0..h {
        square[row * SIDE * 4..][..w * 4].copy_from_slice(&pixels[row * info.line_size..][..w * 4]);
    }
    Ok((square, w as u32, h as u32))
}

unsafe fn upload(player: usize) -> Option<(u32, u32, u32)> {
    let (pixels, w, h) = match decode(player_style(player)?) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("[arcade] crosshair disabled: {error}");
            return None;
        }
    };
    let mut texture = 0;
    unsafe {
        glPushAttrib(ALL_ATTRIB_BITS);
        glPushClientAttrib(CLIENT_PIXEL_STORE_BIT);
        glGenTextures(1, &mut texture);
        glBindTexture(TEXTURE_2D, texture);
        for (name, value) in [(0x0cf5, 1), (0x0cf2, 0), (0x0cf3, 0), (0x0cf4, 0)] {
            glPixelStorei(name, value); // UNPACK_ALIGNMENT, ROW_LENGTH, SKIP_ROWS, SKIP_PIXELS
        }
        for (name, value) in [
            (0x2801, 0x2601),
            (0x2800, 0x2601),
            (0x2802, 0x812f),
            (0x2803, 0x812f),
        ] {
            glTexParameteri(TEXTURE_2D, name, value); // linear, clamp to edge
        }
        glTexImage2D(
            TEXTURE_2D,
            0,
            0x1908, // RGBA
            SIDE as i32,
            SIDE as i32,
            0,
            0x1908,
            0x1401, // UNSIGNED_BYTE
            pixels.as_ptr().cast(),
        );
        glPopClientAttrib();
        glPopAttrib();
    }
    eprintln!("[arcade] player {} crosshair enabled ({w}x{h})", player + 1);
    Some((texture, w, h))
}

/// Draws a player's crosshair over the current frame. Call just before the
/// swap.
pub unsafe fn draw(player: usize, aim_x: c_int, aim_y: c_int, window_w: c_int, window_h: c_int) {
    let player = player.min(TEXTURE_ID.len() - 1);
    let Some((texture, w, h)) = *TEXTURE_ID[player].get_or_init(|| unsafe { upload(player) })
    else {
        return;
    };
    static SIZE: OnceLock<u32> = OnceLock::new();
    let size = *SIZE.get_or_init(|| {
        crosshair::size_percent(std::env::var("ARCADE_CROSSHAIR_SIZE").ok().as_deref())
    });
    let [left, top, right, bottom] =
        crosshair::quad(aim_x, aim_y, w, h, size, window_w, window_h);
    let (s, t) = (w as f32 / SIDE as f32, h as f32 / SIDE as f32);
    unsafe {
        glPushAttrib(ALL_ATTRIB_BITS);
        for mode in [PROJECTION, MODELVIEW, TEXTURE] {
            glMatrixMode(mode);
            glPushMatrix();
            glLoadIdentity();
        }
        glMatrixMode(PROJECTION);
        glOrtho(
            0.0,
            f64::from(window_w),
            f64::from(window_h),
            0.0,
            -1.0,
            1.0,
        );
        glViewport(0, 0, window_w, window_h);
        for cap in DISABLED {
            glDisable(cap);
        }
        glEnable(TEXTURE_2D);
        glEnable(BLEND);
        glBlendFunc(0x0302, 0x0303); // SRC_ALPHA, ONE_MINUS_SRC_ALPHA
        glColorMask(1, 1, 1, 1);
        glPolygonMode(0x0408, 0x1b02); // FRONT_AND_BACK, FILL
        glBindTexture(TEXTURE_2D, texture);
        glTexEnvi(0x2300, 0x2200, 0x2100); // TEXTURE_ENV_MODE = MODULATE
        glColor4f(1.0, 1.0, 1.0, 1.0);
        glBegin(0x0007); // QUADS
        glTexCoord2f(0.0, 0.0);
        glVertex2f(left, top);
        glTexCoord2f(s, 0.0);
        glVertex2f(right, top);
        glTexCoord2f(s, t);
        glVertex2f(right, bottom);
        glTexCoord2f(0.0, t);
        glVertex2f(left, bottom);
        glEnd();
        for mode in [TEXTURE, MODELVIEW, PROJECTION] {
            glMatrixMode(mode);
            glPopMatrix();
        }
        glPopAttrib(); // also restores the matrix mode
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_crosshair_decodes_and_pads() {
        for (id, _, bytes) in crate::crosshair::STYLES {
            let (pixels, w, h) = super::decode(bytes).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!((w, h), (110, 111), "{id}");
            assert_eq!(pixels.len(), super::SIDE * super::SIDE * 4);
            // Visible pixels near the centre dot, transparent padding.
            assert!(pixels[(55 * super::SIDE + 55) * 4 + 3] > 0, "{id}");
            assert_eq!(pixels[(120 * super::SIDE + 120) * 4 + 3], 0);
        }
    }
}
