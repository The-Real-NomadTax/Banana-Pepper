use banana_api::*;
pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    p.dependencies.push(crate::dependencies::libraries());
    if a.id == "setup" {
        p.operation = PlanOperation::PrepareDependencies;
        return Ok(p);
    }
    p.requirements.extend([
        "elf64-dynamic".into(),
        "linux-threads-tls".into(),
        "linux-opengl".into(),
    ]);
    if cfg!(windows) {
        return Ok(p);
    }
    p.input = Some(input_bridge());
    let prepared = crate::image::prepare(&b.bytes)?;
    let image = if a.id == "launch" {
        let records = c.state.join("seeds.json");
        let seeds = std::fs::read(&records).map_err(|e| {
            format!(
                "Captured seed records required ({}): {e}",
                records.display()
            )
        })?;
        let seeds = serde_json::from_slice(&seeds).map_err(|e| e.to_string())?;
        crate::image::seeded(&prepared, &seeds)?
    } else {
        prepared
    };
    p.generated.insert(b.executable.clone(), image.into());
    p.args = vec!["-f1920x1080".into(), "-n".into()];
    p.adapters.push(Adapter {
        artifact: "arcade_minecraft_io".into(),
        destination: ".banana/cards.so".into(),
        preload: true,
    });
    p.env.extend([
        ("ARCADE_MINECRAFT_CARDS".into(), "1".into()),
        ("ARCADE_MINECRAFT_KEYBOARD".into(), "1".into()),
        ("ARCADE_MINECRAFT_CARD".into(), "a0000000".into()),
    ]);
    p.command_channel = Some("actions.sock".into());
    p.env
        .insert("BANANA_ACTION_SOCKET".into(), "/work/actions.sock".into());
    p.env.insert("LD_LIBRARY_PATH".into(),"/pm/.banana/libraries/lib:/pm/.banana/libraries/libraries/usr/lib/x86_64-linux-gnu:/pm/.banana/libraries/libraries/usr/lib/x86_64-linux-gnu/lapack:/pm/.banana/libraries/libraries/usr/lib/x86_64-linux-gnu/blas:/pm/.banana/libraries/libraries/lib/x86_64-linux-gnu".into());
    if c.source.metadata("g7/minecraft").is_ok() {
        p.cwd = "g7/minecraft".into();
        p.aliases
            .insert("g7/minecraft/game".into(), b.executable.clone());
        p.executable = "g7/minecraft/game".into();
    }
    p.requirements.extend([
        "elf64-dynamic".into(),
        "linux-threads-tls".into(),
        "linux-opengl".into(),
    ]);
    Ok(p)
}

fn input_bridge() -> InputBridge {
    let keys: [u16; 38] = [
        2, 3, 4, 5, 6, 7, 8, 9, 44, 45, 46, 47, 105, 106, 103, 108, 23, 36, 37, 38, 22, 24, 25, 39,
        59, 60, 61, 62, 28, 57, 79, 80, 81, 75, 76, 77, 71, 72,
    ];
    let sources: [u16; 38] = [
        30, 31, 32, 33, 34, 35, 36, 37, 29, 27, 6, 25, 80, 79, 82, 81, 12, 13, 14, 15, 24, 18, 19,
        51, 58, 59, 60, 61, 40, 44, 89, 90, 91, 92, 93, 94, 95, 96,
    ];

    InputBridge {
        virtual_pads: 4,
        keys: keys.into_iter().zip(sources).collect(),
        focus_titles: vec!["Minecraft".into(), "gamescope".into()],
    }
}
