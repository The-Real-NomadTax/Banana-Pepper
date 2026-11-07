use banana_api::*;

pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    c.source.metadata("Skylanders_Data/mainData")
        .map_err(|_| "Skylanders_Data/mainData is missing; select the complete DataPC folder")?;
    p.args = player_args(&c.settings);
    p.env.insert("ARCADE_SKYLANDERS_KEYS".into(), c.controls.env_value("skylanders")?);
    p.env.insert("WINEDLLOVERRIDES".into(), format!(
        "PQMTClient,KaboomComDll=n,b;mscoree,mshtml={}",
        if c.settings.audio { "" } else { ";winepulse.drv,winealsa.drv,wineoss.drv=" }
    ));
    // One compiled adapter implements both cabinet interfaces. Each name is
    // memory-backed in the common view, including Unity's plugin directory.
    for destination in ["PQMTClient.dll", "KaboomComDll.dll",
        "Skylanders_Data/Plugins/PQMTClient.dll", "Skylanders_Data/Plugins/KaboomComDll.dll"] {
        p.adapters.push(Adapter {
            artifact: "games/skylanders/windows-x86/PQMTClient.dll".into(),
            destination: destination.into(), preload: false,
        });
    }
    Ok(p)
}
fn player_args(settings: &banana_settings::GameSettings) -> Vec<String> {
    vec!["-screen-width".into(), settings.resolution.0.to_string(),
         "-screen-height".into(), settings.resolution.1.to_string(),
         "-screen-fullscreen".into(), if settings.fullscreen { "1" } else { "0" }.into()]
}
#[cfg(test)]
mod tests {
    #[test]
    fn player_args_follow_settings() {
        let mut settings = banana_settings::GameSettings::defaults(&crate::settings::SETTINGS);
        settings.resolution = (1280, 720);
        settings.fullscreen = true;
        assert_eq!(super::player_args(&settings),
            ["-screen-width", "1280", "-screen-height", "720", "-screen-fullscreen", "1"]);
    }
}
