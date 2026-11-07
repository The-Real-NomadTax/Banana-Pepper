//! Android platform requirements; preparation uses the common compatibility plan.
use banana_api::AndroidProfile;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "game_legacy_patched",
        requirements: &[
            "opt/arcade/games/jurassic/linux-x86/libarcade_jurassic.so",
            "usr/local/bin/box64",
            "usr/local/bin/afbridge",
            "usr/bin/pulseaudio",
            "usr/local/bin/afaudio",
            "opt/arcade/dependencies/legacy32/libPhysXCore.so.1",
            "opt/arcade/dependencies/legacy32/libtheora.so.0",
        ],
        status: "Experimental on Android",
        verified: false,
    }
}
