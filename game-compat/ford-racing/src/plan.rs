use banana_api::*;
pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    if ![
        (640, 480),
        (800, 600),
        (1024, 768),
        (1280, 1024),
        (1600, 1200),
    ]
    .contains(&c.settings.resolution)
    {
        return Err("Unsupported Ford resolution".into());
    }
    let original = String::from_utf8(
        banana_content::read_all(c.source.as_ref(), "data/GameSettings.ini", 1024 * 1024)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut values = std::collections::BTreeMap::from([
        ("Resolution".into(), c.settings.resolution_text()),
        (
            "Fullscreen".into(),
            if c.settings.fullscreen { "on" } else { "off" }.into(),
        ),
    ]);
    for line in original.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().ends_with("Volume") {
                values.insert(
                    key.trim().into(),
                    if c.settings.audio {
                        value.into()
                    } else {
                        "off".into()
                    },
                );
            }
        }
    }
    p.text_configs.push(TextConfig {
        path: "data/GameSettings.ini".into(),
        values,
        newline: "\r\n",
    });
    p.cwd = "FordRacing".into();
    p.adapters.push(Adapter {
        artifact: "inpout32".into(),
        destination: "FordRacing/inpout32.dll".into(),
        preload: false,
    });
    p.env
        .insert("ARCADE_FORD_KEYS".into(), c.controls.env_value("ford")?);
    p.env.insert(
        "WINEDLLOVERRIDES".into(),
        "inpout32=n,b;mscoree,mshtml=".into(),
    );
    Ok(p)
}
