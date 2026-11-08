use banana_api::*;
pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    p.generated.insert(
        b.executable.clone(),
        crate::prepare::transform(
            b.bytes.to_vec(),
            c.source.as_ref(),
            &c.controls.codes("aliens")?,
            &c.settings.input_assignment(),
            &c.settings.aim_modes,
        )?
        .into(),
    );
    p.inert_tools = vec![
        "nvidia-settings".into(),
        "xrandr".into(),
        "reboot".into(),
        "shutdown".into(),
    ];
    p.args.push(format!("-f{}", c.settings.resolution_text()));
    p.env.extend([
        ("ARCADE_ALIENS".into(), "1".into()),
        ("SDL_VIDEO_X11_WMCLASS".into(), "BananaPepperAliens".into()),
        ("SDL_VIDEO_X11_XRANDR".into(), "0".into()),
        ("SDL_VIDEO_X11_VIDMODE".into(), "0".into()),
        ("SDL_VIDEO_X11_DGAMOUSE".into(), "0".into()),
        // The game's statically linked SDL writes Xinerama requests straight into
        // Xlib's Display buffer, which Box32's translated Display (Android) lacks.
        // The game runs in an ordinary window, so the head query is unneeded.
        ("SDL_VIDEO_X11_XINERAMA".into(), "0".into()),
        (
            "LD_LIBRARY_PATH".into(),
            "/pm/.banana/libraries:/usr/lib32".into(),
        ),
    ]);
    p.adapters.push(Adapter {
        artifact: "arcade_aliens".into(),
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
