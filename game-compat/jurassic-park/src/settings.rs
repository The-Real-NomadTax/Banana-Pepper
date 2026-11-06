//! Settings this game's runtime can honour; the shared settings system
//! builds its options and validation from this declaration.
#[allow(unused_imports)]
use banana_settings::{InputMethod, Backend, Crosshair, Profile, Renderer, WindowsAudio};

pub static SETTINGS: Profile = Profile {
        id: "jurassic",
        title: "Jurassic Park 2015",
        default_resolution: (1360, 768),
        default_renderer: Renderer::Gpu,
        resolution_note: "Passed to the game's own -f option. 1360x768 is the cabinet mode and the tested size.",
        crosshair: None,
        max_players: 2,
        input_methods: &InputMethod::ALL,
    backends: &[],
    windows_audio: WindowsAudio::Never,
};
