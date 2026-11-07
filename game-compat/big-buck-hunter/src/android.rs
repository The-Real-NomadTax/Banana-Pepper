//! Android platform requirements; preparation uses the common compatibility plan.
use banana_api::AndroidProfile;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "game",
        requirements: &[
            "opt/arcade/games/big-buck/linux-x86/libarcade_big_buck.so",
            "usr/local/bin/box64",
            "usr/local/bin/afbridge",
        ],
        status: "Experimental mounted Android runtime",
        verified: false,
    }
}
