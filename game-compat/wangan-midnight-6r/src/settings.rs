//! Settings this game's runtime can honour; the shared settings system
//! builds its options and validation from this declaration.
use banana_settings::{Profile, Renderer, WindowsAudio};

pub static SETTINGS: Profile = Profile {
    id: "wmmt6r",
    title: "Wangan Midnight Maximum Tune 6R",
    default_resolution: (1360, 768),
    default_renderer: Renderer::Gpu,
    resolution_note: "Size of the Wine desktop window the cabinet's 1360x768 display runs in; fullscreen uses the whole screen.",
    crosshair: None,
    max_players: 1,
    input_methods: banana_settings::RACING_INPUTS,
    backends: &[],
    windows_audio: WindowsAudio::Never,
};
