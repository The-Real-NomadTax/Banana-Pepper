//! Settings this game's runtime can honour; the shared settings system
//! builds its options and validation from this declaration.
#[allow(unused_imports)]
use banana_settings::{InputMethod, Backend, Crosshair, Profile, Renderer, WindowsAudio};

pub static SETTINGS: Profile = Profile {
    id: "skylanders",
    title: "Skylanders Cloud Patrol",
    default_resolution: (1920, 1080),
    default_renderer: Renderer::Gpu,
    resolution_note: "Passed to the Unity player (-screen-width/-screen-height). The cabinet runs 1920x1080; any size works because the touch adapter follows the window. Renderer: WineD3D (Direct3D 9) on OpenGL (GPU) or Mesa llvmpipe (software).",
    crosshair: None,
    max_players: 1,
    input_methods: banana_settings::RACING_INPUTS,
    backends: &[],
    windows_audio: WindowsAudio::Never,
};
