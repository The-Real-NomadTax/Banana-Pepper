//! Linux controllers for the game adapters: the shared evdev reader
//! (`banana-input/src/evdev.rs`) and the session's input settings, which are
//! platform-neutral (`banana_input::session`).
#[path = "../../../crates/banana-input/src/evdev.rs"]
mod reader;
// Each title uses some of these.
#[allow(unused_imports)]
pub use banana_input::session::{Edges, aim_mode, bindings, bindings_p2, inputs, movement};
pub use reader::Reader;
