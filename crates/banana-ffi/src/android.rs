//! JSON/JNI transport for the Kotlin frontend. All game policy comes from the registry.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing {key}"))
}

fn directories(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), depth)]);
    while let Some((path, remaining)) = queue.pop_front() {
        if out.len() >= 4096 {
            break;
        }
        if !path.is_dir() {
            continue;
        }
        out.push(path.clone());
        if remaining == 0 {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&path) {
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    queue.push_back((entry.path(), remaining - 1));
                }
            }
        }
    }
}

fn source(game: &dyn banana_api::GameCompat, request: &Value) -> Option<PathBuf> {
    let profile = game.android_profile()?;
    let mut dirs = vec![];
    if let Some(path) = request
        .get("sources")
        .and_then(|s| s.get(game.id()))
        .and_then(Value::as_str)
    {
        let path = PathBuf::from(path);
        if path.join(profile.executable).is_file() {
            return Some(path);
        }
    }
    for root in request
        .get("roots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        directories(Path::new(root), 3, &mut dirs);
    }
    for dir in dirs {
        let executable = dir.join(profile.executable);
        if !executable.is_file() {
            continue;
        }
        // Ambiguous names such as `game` are identified by the same fingerprints
        // used by the desktop. No title is inferred from a filename alone.
        if game.known_builds().is_empty() {
            if game.locate_executable(&dir).is_file() || profile.executable.ends_with(".exe") {
                return Some(dir);
            }
        } else if fs::read(&executable).is_ok_and(|bytes| {
            let hash = format!("{:x}", Sha256::digest(bytes));
            game.known_builds().iter().any(|build| build.sha256 == hash)
        }) {
            return Some(dir);
        }
    }
    None
}

fn missing(game: &dyn banana_api::GameCompat, files: &Path) -> Vec<String> {
    let Some(profile) = game.android_profile() else {
        return vec!["Android backend".into()];
    };
    let mut missing: Vec<String> = profile
        .requirements
        .iter()
        .filter(|relative| {
            let host = if let Some(tool) = relative.strip_prefix("opt/arcade/") {
                files.join("opt").join(tool)
            } else {
                files.join("rootfs").join(relative)
            };
            !host.is_file()
        })
        .map(|s| (*s).into())
        .collect();
    for (relative, label) in [
        ("bin/proot", "Android proot"),
        ("bin/loader", "Android proot loader"),
        ("opt/run-game.sh", "Android session runner"),
        ("rootfs/usr/bin/Xvfb", "X11 runtime"),
        ("rootfs/usr/bin/bash", "Linux shell"),
    ] {
        if !files.join(relative).is_file() {
            missing.push(label.into());
        }
    }
    missing
}

fn catalog(request: &Value) -> Result<Value, String> {
    let files = Path::new(required(request, "files")?);
    let games: Vec<_> = banana_api::games().iter().filter(|game|game.android_profile().is_some()).map(|game| {
        let source = source(*game,request);
        let profile = game.android_profile();
        let missing = missing(*game,files);
        let controls: Vec<_> = game.control_layout().into_iter().flat_map(|p|p.actions).filter_map(|a| {
            let key = a.default.iter().find(|s|!s.starts_with("Pad") && !s.starts_with("Mouse"))?;
            Some(json!({"id":a.id,"label":a.label,"key": match *key {"Enter"=>"Return".into(),"Space"=>"space".into(),"Backspace"=>"BackSpace".into(),"PageUp"=>"Prior".into(),"PageDown"=>"Next".into(), other if other.len()==1=>other.to_lowercase(),other=>other.into()}}))
        }).collect();
        json!({"id":game.id(),"title":game.title(),"source":source,"found":source.is_some(),
               "controls":controls,"kind":game.controls(),
               "status":profile.as_ref().map(|p|p.status).unwrap_or("Android unavailable"),
               "verified":profile.as_ref().is_some_and(|p|p.verified),"missing":missing,
               "ready":source.is_some() && missing.is_empty(),"executable":profile.map(|p|p.executable)})
    }).collect();
    Ok(json!(games))
}

fn prepare(request: &Value) -> Result<Value, String> {
    let game = banana_api::game(required(request, "id")?).ok_or("Unknown game")?;
    let files = Path::new(required(request, "files")?);
    let missing = missing(game, files);
    if !missing.is_empty() {
        return Err(format!(
            "Missing runtime components: {}",
            missing.join(", ")
        ));
    }
    let source = Path::new(required(request, "source")?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let work = files.join("sessions").join(game.id());
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut plan = game.prepare_android(&source, &work, &files.join("opt"))?;
    // Always route Android touch through the guest's keyboard/mouse path.
    plan.env("ARCADE_ANDROID", "1");
    plan.env("ARCADE_INPUTS", "keyboard_mouse");
    plan.env("ARCADE_PLAYERS", "1");
    if !plan.environment.iter().any(|(key, _)| key == "ARCADE_RESOLUTION") {
        plan.env("ARCADE_RESOLUTION", "640x480");
    }
    let tag = request
        .get("session_tag")
        .and_then(Value::as_str)
        .unwrap_or("");
    if tag.len() > 64 || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("Invalid session tag".into());
    }
    let run = if tag.is_empty() {
        files.join("run")
    } else {
        files.join("run").join(tag)
    };
    let tmp = if tag.is_empty() {
        files.join("tmp")
    } else {
        files.join("tmp").join(tag)
    };
    fs::create_dir_all(&run).map_err(|e| e.to_string())?;
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let bin = files.join("bin");
    let mut command = vec![
        bin.join("proot").display().to_string(),
        "--kill-on-exit".into(),
        "--link2symlink".into(),
        "-0".into(),
        "-r".into(),
        files.join("rootfs").display().to_string(),
    ];
    for (host, guest) in [
        (PathBuf::from("/dev"), "/dev"),
        (PathBuf::from("/proc"), "/proc"),
        (PathBuf::from("/sys"), "/sys"),
        (tmp.clone(), "/tmp"),
        (tmp.clone(), "/dev/shm"),
        (run.clone(), "/run/af"),
        (files.join("opt"), "/opt/arcade"),
    ] {
        command.extend(["-b".into(), format!("{}:{guest}", host.display())]);
    }
    for (host, guest) in plan.binds {
        command.extend(["-b".into(), format!("{host}:{guest}")]);
    }
    command.extend([
        "-w".into(),
        plan.directory,
        "/usr/bin/env".into(),
        "-i".into(),
        "PATH=/usr/local/bin:/usr/bin:/bin".into(),
        "LANG=C.UTF-8".into(),
    ]);
    for (key, value) in plan.environment {
        command.push(format!("{key}={value}"));
    }
    for key in ["AF_INPUT_SOCKET", "AF_AUDIO_SOCKET", "AF_APP_PID"] {
        if let Some(value) = request.get(key).and_then(Value::as_str) {
            command.push(format!("{key}={value}"));
        }
    }
    command.extend(["/bin/bash".into(), "/opt/arcade/run-game.sh".into()]);
    command.extend(plan.command);
    Ok(
        json!({"command":command,"environment":{"LD_LIBRARY_PATH":bin,"PROOT_LOADER":bin.join("loader"),"PROOT_TMP_DIR":tmp},"run":run}),
    )
}

/// One transport entrypoint, also exercised by host tests.
pub fn dispatch(operation: &str, payload: &str) -> String {
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        crate::initialize();
        let request: Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
        match operation {
            "catalog" => catalog(&request),
            "prepare" => prepare(&request),
            _ => Err("Unknown backend operation".into()),
        }
    })
    .unwrap_or_else(|_| Err("The Rust backend could not complete the request".into()));
    match result {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(error) => json!({"ok":false,"error":error}),
    }
    .to_string()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_bananapepper_RustBackend_request(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    operation: jni::objects::JString,
    payload: jni::objects::JString,
) -> jni::sys::jstring {
    let result = (|| {
        let op: String = env.get_string(&operation).ok()?.into();
        let payload: String = env.get_string(&payload).ok()?.into();
        env.new_string(dispatch(&op, &payload))
            .ok()
            .map(|s| s.into_raw())
    })();
    result.unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_covers_registry_without_advertising_missing_runtimes() {
        let v: Value =
            serde_json::from_str(&dispatch("catalog", r#"{"files":"/missing","roots":[]}"#))
                .unwrap();
        let games = v["value"].as_array().unwrap();
        assert_eq!(
            games.len(),
            banana_api::games()
                .iter()
                .filter(|g| g.android_profile().is_some())
                .count()
        );
        assert!(
            games
                .iter()
                .all(|g| g["ready"] == false && !g["missing"].as_array().unwrap().is_empty())
        );
        assert_eq!(games.iter().filter(|g| g["verified"] == true).count(), 1);
    }
    #[test]
    fn unknown_and_malformed_requests_are_errors() {
        for (op, payload) in [
            ("prepare", "{}"),
            ("catalog", "broken"),
            ("unknown", "{}"),
            ("prepare", r#"{"id":"absent","files":"/tmp"}"#),
        ] {
            let value: Value = serde_json::from_str(&dispatch(op, payload)).unwrap();
            assert_eq!(value["ok"], false);
        }
    }
}
