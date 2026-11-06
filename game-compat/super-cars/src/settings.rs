//! Settings this game's runtime can honour; the shared settings system
//! builds its options and validation from this declaration.
#[allow(unused_imports)]
use banana_settings::{Backend, Crosshair, Profile, Renderer, WindowsAudio};

pub static SETTINGS: Profile = Profile {
        id: "super_cars",
        title: "Fast and Furious Super Cars",
        default_resolution: (1920, 1080),
        default_renderer: Renderer::Gpu,
        resolution_note: "Passed to the game's own -res option: 640x480, 800x600, 1024x768, 1280x1024, 1360x768, 1600x1200 or 1920x1080. Also the Wine window size. Renderer: WineD3D on OpenGL (GPU) or Mesa llvmpipe (software).",
        crosshair: None,
        max_players: 1,
    backends: &[],
    windows_audio: WindowsAudio::Never,
};
