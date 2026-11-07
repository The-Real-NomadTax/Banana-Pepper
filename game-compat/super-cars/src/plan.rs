use banana_api::*;
pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    let flag = match c.settings.resolution {
        (640, 480) => "-res6",
        (800, 600) => "-res8",
        (1024, 768) => "-res10",
        (1280, 1024) => "-res12",
        (1360, 768) => "-res13",
        (1600, 1200) => "-res16",
        (1920, 1080) => "-res19",
        _ => return Err("Unsupported Super Cars resolution".into()),
    };
    p.args = vec!["-noerr".into(), flag.into()];
    p.guest_root = "C:/rawart".into();
    p.env.insert(
        "ARCADE_SUPERCARS_KEYS".into(),
        c.controls.env_value("super_cars")?,
    );
    p.env.insert(
        "WINEDLLOVERRIDES".into(),
        format!(
            "dinput8=n,b;mscoree,mshtml=;d3d8,d3d9=b{}",
            if c.settings.audio {
                ""
            } else {
                ";winepulse.drv,winealsa.drv="
            }
        ),
    );
    p.adapters.push(Adapter {
        artifact: "games/super-cars/windows-x86/arcade_wine_hook.dll".into(),
        destination: "dinput8.dll".into(),
        preload: false,
    });
    Ok(p)
}
