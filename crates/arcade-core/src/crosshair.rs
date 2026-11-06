//! On-screen crosshairs for the gun games (Big Buck Hunter), drawn by the
//! guest adapters over each frame at the current aim position.
//!
//! The images are 110x111 RGBA PNGs centred on the aim point and drawn 1:1 in
//! the game's 640x480 coordinate space (scaled with the window). The style id
//! reaches the adapters as `ARCADE_CROSSHAIR` (from settings.cfg).
//!
//! No `std` use, so the Linux guest shim can include it with `#[path]`.

/// (id, label, PNG bytes). Player-one red and player-two blue, each with a
/// larger version.
pub const STYLES: &[(&str, &str, &[u8])] = &[
    (
        "p1",
        "Red (P1)",
        include_bytes!("../assets/crosshairs/p1.png"),
    ),
    (
        "p1_large",
        "Red, large (P1)",
        include_bytes!("../assets/crosshairs/p1_large.png"),
    ),
    (
        "p2",
        "Blue (P2)",
        include_bytes!("../assets/crosshairs/p2.png"),
    ),
    (
        "p2_large",
        "Blue, large (P2)",
        include_bytes!("../assets/crosshairs/p2_large.png"),
    ),
];

/// Size as a percentage of the images' own size (`crosshair_size`).
pub const SIZE_MIN: u32 = 10;
pub const SIZE_MAX: u32 = 200;
pub const SIZE_DEFAULT: u32 = 50;

/// `ARCADE_CROSSHAIR_SIZE` value: a percentage, clamped; default if absent/bad.
pub fn size_percent(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.trim().trim_end_matches('%').parse::<u32>().ok())
        .map_or(SIZE_DEFAULT, |v| v.clamp(SIZE_MIN, SIZE_MAX))
}

/// The image for a style id, or `None` for "off"/unknown.
pub fn png(id: &str) -> Option<&'static [u8]> {
    STYLES.iter().find(|s| s.0 == id).map(|s| s.2)
}

/// Window-space quad (left, top, right, bottom) for an image of
/// `width`x`height` game pixels at `percent` size, centred on game position
/// (`x`, `y`), in a window of `window_w`x`window_h` showing the 640x480 game.
pub fn quad(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    percent: u32,
    window_w: i32,
    window_h: i32,
) -> [f32; 4] {
    let scale = percent as f32 / 100.0;
    let sx = window_w.max(1) as f32 / 640.0 * scale;
    let sy = window_h.max(1) as f32 / 480.0 * scale;
    let (wx, wy) = (window_w.max(1) as f32 / 640.0, window_h.max(1) as f32 / 480.0);
    let (cx, cy) = ((x as f32 + 0.5) * wx, (y as f32 + 0.5) * wy);
    let (hw, hh) = (width as f32 * sx / 2.0, height as f32 * sy / 2.0);
    [cx - hw, cy - hh, cx + hw, cy + hh]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_is_a_png() {
        for (id, _, bytes) in STYLES {
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{id}");
        }
        assert!(png("p2_large").is_some());
        assert!(png("off").is_none());
    }

    #[test]
    fn quads_scale_with_the_window() {
        assert_eq!(
            quad(320, 240, 110, 110, 100, 640, 480),
            [265.5, 185.5, 375.5, 295.5]
        );
        assert_eq!(
            quad(0, 0, 100, 100, 100, 1280, 960),
            [-99.0, -99.0, 101.0, 101.0]
        );
        // Half size keeps the centre.
        assert_eq!(
            quad(320, 240, 100, 100, 50, 640, 480),
            [295.5, 215.5, 345.5, 265.5]
        );
    }

    #[test]
    fn size_is_a_clamped_percentage() {
        assert_eq!(size_percent(None), SIZE_DEFAULT);
        assert_eq!(size_percent(Some("75")), 75);
        assert_eq!(size_percent(Some("75%")), 75);
        assert_eq!(size_percent(Some("1")), SIZE_MIN);
        assert_eq!(size_percent(Some("900")), SIZE_MAX);
        assert_eq!(size_percent(Some("big")), SIZE_DEFAULT);
    }
}
