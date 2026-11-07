//! Linux frontend: preloaded into the game. It exports the shared GLUT
//! overrides (`glut.rs`), reaches the real GLUT through `RTLD_NEXT` and
//! reads controllers through evdev.
use crate::{glut, pad};
use std::ffi::{CStr, c_int};

/// Controllers through evdev.
pub struct Devices(crate::evdev_pad::Reader);
impl Devices {
    pub fn new(_inputs: &pad::Inputs) -> Self {
        Self(crate::evdev_pad::Reader::new())
    }
    pub fn read_players(&mut self, inputs: &pad::Inputs) -> [pad::Pad; pad::MAX_PLAYERS] {
        self.0.read_players(inputs)
    }
}

/// The next definition of a library function (the real GLUT).
pub fn next(name: &CStr) -> usize {
    unsafe { libc::dlsym(libc::RTLD_NEXT, name.as_ptr()) as usize }
}

/// evdev reads every controller regardless of window focus.
pub fn focused() -> bool {
    true
}

/// Nothing to add to the window on Linux.
pub fn window_created() {}

/// Install the virtual JAMMA API when the launch plan asked for it.
pub fn install() {
    #[cfg(target_arch = "x86")]
    if std::env::var_os("ARCADE_BBH_ADAPTER").is_some_and(|v| v == "1") {
        if let Err(e) = unsafe { crate::jamma::install() } {
            eprintln!("[arcade] {e}; adapter disabled");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn glutEnterGameMode() -> c_int {
    glut::enter_game_mode()
}
#[unsafe(no_mangle)]
pub extern "C" fn glutLeaveGameMode() {
    glut::leave_game_mode()
}
#[unsafe(no_mangle)]
pub extern "C" fn glutSwapBuffers() {
    glut::swap_buffers()
}

/// Patch the game before its own initialization runs.
extern "C" fn initialize() {
    install();
}
#[used]
#[unsafe(link_section = ".init_array")]
static INITIALIZE: extern "C" fn() = initialize;
