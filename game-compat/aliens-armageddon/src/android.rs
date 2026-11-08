//! Android platform requirements; preparation uses the common compatibility plan.
use banana_api::AndroidProfile;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "game",
        requirements: &[
            "opt/arcade/games/aliens/linux-x86/libarcade_aliens.so",
            "usr/local/bin/box64",
            "usr/bin/Xvfb",
            "usr/bin/pulseaudio",
            "opt/arcade/dependencies/legacy32/libPhysXCore.so.1",
        ],
        status: "Experimental on Android",
        verified: false,
    }
}
