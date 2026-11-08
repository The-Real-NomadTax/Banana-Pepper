//! Android platform requirements; preparation uses the common compatibility plan.
use banana_api::AndroidProfile;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "FordRacing/fordracing.exe",
        requirements: &[
            "opt/arcade/games/ford/windows-x86/inpout32.dll",
            "usr/local/bin/box64",
            "usr/bin/Xvfb",
            "usr/bin/pulseaudio",
            "opt/arcade/wine/bin/wine",
            "opt/arcade/x64-libs/libgcc_s.so.1",
            "opt/arcade/wine/lib/wine/x86_64-unix/ntdll.so",
        ],
        status: "Experimental Wine WoW64 runtime",
        verified: false,
    }
}
