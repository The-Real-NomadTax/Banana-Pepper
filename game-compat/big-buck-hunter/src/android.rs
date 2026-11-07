use banana_api::{AndroidLaunch, AndroidProfile};
use std::path::Path;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "game",
        requirements: &["usr/local/bin/box64", "usr/local/bin/afbridge"],
        status: "Android gameplay verified",
        verified: true,
    }
}

pub fn prepare(source: &Path, work: &Path, _tools: &Path) -> Result<AndroidLaunch, String> {
    crate::prepare::prepare(&source.join("game"), &work.join("game"))?;
    let saves = work.join("bbh3user");
    if let Some(files) = _tools.parent() {
        banana_runtime::android::seed(&files.join("saves/bbh3user"), &saves)?;
    }
    banana_runtime::android::seed(&source.join("bbh3user"), &saves)?;
    let mut plan = AndroidLaunch::new("/game", &["box64", "/game/game"]);
    plan.bind(source, "/game");
    plan.bind(work.join("game"), "/game/game");
    plan.bind(source.join("bbh3data"), "/bbh3data");
    plan.bind(saves, "/bbh3user");
    plan.env("BOX64_LD_PRELOAD", "/opt/arcade/libarcade_linux_shim.so");
    plan.env("ARCADE_BBH_ADAPTER", "1");
    plan.env("ARCADE_BBH_AUDIO", "1");
    Ok(plan)
}
