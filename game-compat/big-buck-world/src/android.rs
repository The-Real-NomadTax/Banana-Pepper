//! Android platform requirements; preparation uses the common compatibility plan.
use banana_api::AndroidProfile;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "game",
        requirements: &[
            "opt/arcade/games/big-buck-world/linux-x86/libarcade_big_buck_world.so",
            "usr/local/bin/box64",
            "usr/bin/Xvfb",
            "opt/arcade/dependencies/big-buck-world/libstdc++.so.5",
        ],
        status: "Experimental mounted Android runtime",
        verified: false,
    }
}
