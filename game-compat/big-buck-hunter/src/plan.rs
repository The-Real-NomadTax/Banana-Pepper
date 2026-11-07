use banana_api::*;
pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    p.generated.insert(
        b.executable.clone(),
        crate::prepare::transform(b.bytes.to_vec())?.into(),
    );
    p.env
        .insert("ARCADE_BBH_KEYS".into(), c.controls.env_value("big_buck")?);
    p.guest_root = "/game".into();
    p.env.insert("ARCADE_BBH_ADAPTER".into(), "1".into());
    p.guest_mounts.extend([
        GuestMount {
            source: "bbh3data".into(),
            target: "/bbh3data".into(),
            readonly: true,
        },
        GuestMount {
            source: "bbh3user".into(),
            target: "/bbh3user".into(),
            readonly: false,
        },
    ]);
    p.requirements.push("elf32-fixed-imports".into());
    if a.id == "probe" {
        p.timeout_ms = Some(96000);
        p.env.extend([
            ("ARCADE_NATIVE_PROBE".into(), "1".into()),
            ("ARCADE_AUDIO".into(), "0".into()),
            ("ARCADE_RESOLUTION".into(), "640x480".into()),
            ("ARCADE_FULLSCREEN".into(), "0".into()),
        ]);
    }
    // The same patch on every host: Water loads it beside the game on
    // Windows; elsewhere it is preloaded into the game.
    if cfg!(windows) {
        p.adapters.push(Adapter {
            artifact: "arcade_big_buck".into(),
            destination: ".banana/water-adapter.dll".into(),
            preload: false,
        });
    } else {
        p.adapters.push(Adapter {
            artifact: "arcade_big_buck".into(),
            destination: ".banana/guest.so".into(),
            preload: true,
        });
        p.dependencies.push(Dependency {
            id: "legacy32".into(),
            destination: ".banana/libraries".into(),
            recipe: None,
        });
        p.env.insert(
            "LD_LIBRARY_PATH".into(),
            "/game/.banana/libraries:/usr/lib32".into(),
        );
    }
    Ok(p)
}
