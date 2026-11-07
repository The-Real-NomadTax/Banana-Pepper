use banana_api::*;
pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    if b.sha256 != crate::prepare::Build::Native.sha256() {
        return Err("Desktop Jurassic requires the recognized legacy build".into());
    }
    p.generated.insert(
        b.executable.clone(),
        crate::prepare::transform(b.bytes.to_vec(), crate::prepare::Build::Native, true)?.into(),
    );
    p.aliases.insert("game".into(), b.executable.clone());
    validate_programs(c.source.as_ref(), "g5/dino/data/programs_enc")?;
    p.aliases
        .insert("g5/dino/data/programs_enc".into(), "programs_dec".into());
    p.aliases.insert("g5/dino/pmuser".into(), "pmuser".into());
    p.env.extend([
        ("ARCADE_JURASSIC".into(), "1".into()),
        ("ARCADE_JURASSIC_LAUNCH".into(), "1".into()),
        ("ARCADE_JURASSIC_DESKTOP".into(), "1".into()),
        (
            "ARCADE_JURASSIC_KEYS".into(),
            c.controls.env_value("jurassic")?,
        ),
        ("LD_BIND_NOW".into(), "1".into()),
        ("SDL12COMPAT_OPENGL_SCALING".into(), "0".into()),
        ("LD_LIBRARY_PATH".into(), "/pm/.banana/libraries".into()),
    ]);
    shaders(c.source.as_ref(), &mut p)?;
    p.inert_tools.push("nvidia-settings".into());
    p.generated.insert(
        ".banana/usb.rules".into(),
        std::sync::Arc::from(&b"# Cabinet automount is disabled in this private namespace.\n"[..]),
    );
    p.guest_mounts.push(GuestMount {
        source: ".banana/usb.rules".into(),
        target: "/etc/udev/rules.d/10-usb-automount.rules".into(),
        readonly: true,
    });
    p.guest_mounts.push(GuestMount {
        source: "pmuser".into(),
        target: "/pmuser".into(),
        readonly: false,
    });
    p.executable = "game".into();
    p.args.push(format!("-f{}", c.settings.resolution_text()));
    p.adapters.push(Adapter {
        artifact: "games/jurassic/linux-x86/libarcade_jurassic.so".into(),
        destination: ".banana/guest.so".into(),
        preload: true,
    });
    p.dependencies.push(Dependency {
        id: "legacy32".into(),
        destination: ".banana/libraries".into(),
        recipe: None,
    });
    p.requirements.push("linux-sdl12".into());
    Ok(p)
}
fn validate_programs(s: &dyn banana_content::ContentSource, dir: &str) -> Result<(), String> {
    let entries = s.read_dir(dir).map_err(|e| e.to_string())?;
    if entries.is_empty() {
        return Err("No Jurassic program files found".into());
    }
    for e in entries {
        let p = format!("{dir}/{}", e.name);
        if e.metadata.directory {
            validate_programs(s, &p)?
        } else if p.ends_with(".txt") {
            let backup = p.replacen("g5/dino/data/programs_enc", "programs_dec", 1);
            let f = s
                .open(&backup)
                .map_err(|e| format!("Missing plaintext backup {backup}: {e}"))?;
            let mut b = [0; 3];
            f.read_at(0, &mut b).map_err(|e| e.to_string())?;
            if &b != b"//\n" {
                return Err(format!("Unsupported plaintext backup {backup}"));
            }
        }
    }
    Ok(())
}

fn shaders(s: &dyn banana_content::ContentSource, p: &mut LaunchPlan) -> Result<(), String> {
    let prefix = "out vec4 out_Color;\r\n#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\r\n#define ARCADE_BLOOM_OUTPUT_DECLARED\r\nout vec4 out_Bloom;\r\n#endif\r\n";
    for n in [
        "shader108",
        "shader108b",
        "shader108c_sk",
        "shader108d",
        "shader108d_sk",
    ] {
        let path = format!("g5/shaders/{n}/frag.gls");
        let source = String::from_utf8(
            banana_content::read_all(s, &path, 4 * 1024 * 1024).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if !source.contains("gl_FragData[0]") || !source.contains("gl_FragData[1]") {
            return Err(format!("Unsupported shader variant {path}"));
        }
        p.generated.insert(
            path,
            format!(
                "{prefix}{}",
                source
                    .replace("gl_FragData[0]", "out_Color")
                    .replace("gl_FragData[1]", "out_Bloom")
            )
            .into_bytes()
            .into(),
        );
    }
    let path = "g5/shaders/include/frag_bloom.gls";
    let source = String::from_utf8(
        banana_content::read_all(s, path, 4 * 1024 * 1024).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if source.matches("out vec4 out_Bloom;").count() != 1 {
        return Err("Unsupported bloom shader".into());
    }
    p.generated.insert(path.into(),source.replace("out vec4 out_Bloom;","#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\r\n#define ARCADE_BLOOM_OUTPUT_DECLARED\r\nout vec4 out_Bloom;\r\n#endif").into_bytes().into());
    Ok(())
}
