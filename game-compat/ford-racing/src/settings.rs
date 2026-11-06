//! Settings this game's runtime can honour; the shared settings system
//! builds its options and validation from this declaration.
#[allow(unused_imports)]
use banana_settings::{Backend, Crosshair, Profile, Renderer, WindowsAudio};

pub static SETTINGS: Profile = Profile {id:"ford",title:"Ford Racing Full Blown",default_resolution:(640,480),default_renderer:Renderer::Gpu,resolution_note:"Native Direct3D 9 on Windows; Wine on Linux. Cabinet resolutions: 640x480, 800x600, 1024x768, 1280x1024, 1600x1200. Default is windowed.",crosshair:None,max_players:1,
    backends: &[],
    windows_audio: WindowsAudio::Always,
};
