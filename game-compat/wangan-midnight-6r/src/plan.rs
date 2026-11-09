use banana_api::*;

/// The cabinet keeps operator settings, credits and backups on drives F:
/// (primary) and G: (backup), and its debug logs on J:. All are served from
/// this source folder.
const BACKUP: &str = "TP";
/// The CA certificate that terminal connections must chain to.
const CA_FILE: &str = "data_jp/network/certs/v388-ca-cert.pem";

pub fn make(b: &IdentifiedBuild, c: &PlanContext, a: &ActionRequest) -> Result<LaunchPlan, String> {
    let mut p = LaunchPlan::new(&crate::GAME, b, a)?;
    let probe = a.id == "probe";
    p.generated.insert(
        b.executable.clone(),
        crate::image::prepare(&b.bytes, probe)?.into(),
    );
    // The game trusts its terminal only through this CA file; the emulated
    // terminal's certificate is signed by the emulation's own CA.
    let folder = b.executable.rsplit_once('/').map_or("", |(dir, _)| dir);
    let ca = if folder.is_empty() { CA_FILE.to_string() } else { format!("{folder}/{CA_FILE}") };
    let mut pem = vec![0u8; 1200];
    let n = arcade_wmmt6r::pki::ca_pem(&mut pem);
    pem.truncate(n);
    p.generated.insert(ca, pem.into());
    p.adapters.push(Adapter {
        artifact: "arcade_wmmt6r".into(),
        destination: crate::image::HOOK_LIBRARY.into(),
        preload: false,
    });
    if !c.source.metadata(BACKUP).is_ok_and(|m| m.directory) {
        return Err(format!(
            "The {BACKUP} folder (the cabinet's F:/G: backup drive with setting.lua.gz) is missing from the game folder"
        ));
    }
    // D: is the game folder itself, so Wine gives the game DOS paths.
    for (drive, source) in [("D:", ""), ("F:", BACKUP), ("G:", BACKUP), ("J:", BACKUP)] {
        p.guest_mounts.push(GuestMount {
            source: source.into(),
            target: drive.into(),
            readonly: false,
        });
    }
    p.env.insert(
        "ARCADE_WMMT6R_KEYS".into(),
        c.controls.env_value(crate::GAME.config_key())?,
    );
    p.env.insert(
        "WINEDLLOVERRIDES".into(),
        format!(
            "bngrw=n;mscoree,mshtml={}",
            if c.settings.audio {
                ""
            } else {
                ";winepulse.drv,winealsa.drv="
            }
        ),
    );
    if probe {
        // The game's own log goes to OutputDebugString (see image::PATCHES),
        // which Wine reports on its exception channel.
        p.env
            .insert("WINEDEBUG".into(), "err+all,warn+seh".into());
        p.env.insert("ARCADE_WMMT6R_TRACE".into(), "1".into());
        p.timeout_ms = Some(600_000);
    }
    Ok(p)
}
