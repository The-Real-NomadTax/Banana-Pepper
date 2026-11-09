//! Settings this game's runtime can honour; the shared settings system
//! builds its options and validation from this declaration.
#[allow(unused_imports)]
use banana_settings::{Backend, Crosshair, InputMethod, Profile, Renderer, WindowsAudio};

pub static SETTINGS: Profile = Profile {
    id: "big_buck_world",
    title: "Big Buck World",
    default_resolution: (640, 480),
    default_renderer: if cfg!(windows) {
        Renderer::Gpu
    } else {
        Renderer::Software
    },
    resolution_note: "Window size. The game still renders at 640x480; aiming is rescaled to the window.",
    crosshair: Some(Crosshair(Some("p1"))),
    max_players: 2,
    input_methods: &InputMethod::ALL,
    backends: &[Backend::Auto, Backend::Water],
    windows_audio: WindowsAudio::Always,
};
