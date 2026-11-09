use banana_api::*;
pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    if !cfg!(windows) {
        p.dependencies.push(Dependency {
            id: "big-buck-world".into(),
            destination: ".banana/world-libraries".into(),
            recipe: Some(DependencyRecipe {
                deb: &[],
                downloads: vec![DownloadSpec {
                    url: "https://deb.debian.org/debian/pool/main/g/gcc-3.3/libstdc++5_3.3.6-34_i386.deb",
                    sha256: "55d14a8e77551f500b61893a006d960872a35a29229c782acfa7f8cd48d21063",
                    limit: 1024 * 1024,
                    members: vec![
                        ("./usr/lib/i386-linux-gnu/libstdc++.so.5.0.7", "libstdc++.so.5"),
                        ("./usr/share/doc/libstdc++5/copyright", "libstdc++5-copyright"),
                    ],
                }],
            }),
        });
    }
    if a.id == "setup" {
        p.operation = PlanOperation::PrepareDependencies;
        return Ok(p);
    }
    p.generated.insert(
        b.executable.clone(),
        crate::prepare::transform(b.bytes.to_vec())?.into(),
    );
    p.env.insert(
        "ARCADE_BBH_KEYS".into(),
        c.controls.env_value("big_buck_world")?,
    );
    p.guest_root = "/game".into();
    p.env.insert("ARCADE_BBH_ADAPTER".into(), "1".into());
    p.guest_mounts.extend([
        GuestMount {
            source: "bbwdata".into(),
            target: "/g3/bbwdata".into(),
            readonly: true,
        },
        GuestMount {
            source: "bbwuser".into(),
            target: "/bbwuser".into(),
            readonly: false,
        },
    ]);
    p.requirements.push("elf32-fixed-imports".into());
    if a.id == "probe" {
        p.timeout_ms = Some(480000);
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
            artifact: "arcade_big_buck_world".into(),
            destination: ".banana/water-adapter.dll".into(),
            preload: false,
        });
    } else {
        p.adapters.push(Adapter {
            artifact: "arcade_big_buck_world".into(),
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
            "/game/.banana/world-libraries:/game/.banana/libraries:/usr/lib32".into(),
        );
    }
    Ok(p)
}
