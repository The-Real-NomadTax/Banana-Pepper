//! JSON/JNI transport for the Kotlin frontend. All game policy comes from the registry.
use serde_json::{Value, json};
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
    game.android_profile()?;
    let recognized = |path: &Path| {
        banana_content::open(path)
            .ok()
            .and_then(|source| game.identify(source.as_ref()).ok())
            .is_some_and(|build| build.known)
    };
    if let Some(path) = request
        .get("sources")
        .and_then(|s| s.get(game.id()))
        .and_then(Value::as_str)
    {
        let path = PathBuf::from(path);
        if recognized(&path) {
            return Some(path);
        }
    }
    let mut dirs = Vec::new();
    for root in request
        .get("roots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let path = Path::new(root);
        if path.is_file() && recognized(path) {
            return Some(path.to_path_buf());
        }
        directories(path, 3, &mut dirs);
    }
    for dir in dirs {
        if recognized(&dir) {
            return Some(dir);
        }
        for entry in fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            if entry
                .path()
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("zar"))
                && recognized(&entry.path())
            {
                return Some(entry.path());
            }
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
    missing.extend(banana_android::missing_runtime(files));
    missing
}

fn catalog(request: &Value) -> Result<Value, String> {
    let files = Path::new(required(request, "files")?);
    let bindings = banana_input::controls::Bindings::load(&files.join("config/controls.cfg"))?;
    let games: Vec<_> = banana_api::games().iter().filter(|game|game.android_profile().is_some()).map(|game| {
        let source = source(*game,request);
        let profile = game.android_profile();
        let missing = missing(*game,files);
        let controls: Vec<_> = game.control_layout().into_iter().flat_map(|p|p.actions).filter_map(|a| {
            let key = bindings.keys(game.config_key(),a.id).iter().find(|s|!s.starts_with("Pad") && !s.starts_with("Mouse"))?;
            Some(json!({"id":a.id,"label":a.label,"key": match key.as_str() {"Enter"=>"Return".into(),"Space"=>"space".into(),"Backspace"=>"BackSpace".into(),"PageUp"=>"Prior".into(),"PageDown"=>"Next".into(), other if other.len()==1=>other.to_lowercase(),other=>other.into()}}))
        }).collect();
        json!({"id":game.id(),"title":game.title(),"source":source,"found":source.is_some(),
               "controls":controls,"kind":game.controls(),
               "status":profile.as_ref().map(|p|p.status).unwrap_or("Android unavailable"),
               "verified":profile.as_ref().is_some_and(|p|p.verified),"missing":missing,
               "ready":source.is_some() && missing.is_empty(),"executable":profile.map(|p|p.executable)})
    }).collect();
    Ok(json!(games))
}

#[cfg(target_os = "android")]
struct Running {
    session: Option<banana_runtime::Session>,
    exit: Option<i32>,
}
#[cfg(target_os = "android")]
fn sessions() -> &'static std::sync::Mutex<std::collections::BTreeMap<u64, Running>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<u64, Running>>> =
        std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}
fn start(request: &Value) -> Result<Value, String> {
    let game = banana_api::game(required(request, "id")?).ok_or("Unknown game")?;
    let source = Path::new(required(request, "source")?);
    let content = banana_content::open(source).map_err(|e| e.to_string())?;
    let build = game.identify(content.as_ref())?;
    if !build.known {
        return Err("Unrecognized Android game build; no transforms applied".into());
    }
    let files = Path::new(required(request, "files")?);
    let absent = missing(game, files);
    if !absent.is_empty() {
        return Err(format!("Missing runtime components: {}", absent.join(", ")));
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Android execution is available only in the Android application".into())
    }
    #[cfg(target_os = "android")]
    {
        let tag = request
            .get("session_tag")
            .and_then(Value::as_str)
            .unwrap_or("default");
        if tag.is_empty()
            || tag.len() > 64
            || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("Invalid session tag".into());
        }
        let paths = banana_runtime::storage::Paths {
            config: files.join("config"),
            data: files.join("data"),
            state: files.join("state"),
            cache: request
                .get("cache")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| files.join("cache")),
        };
        let run = files.join("run").join(tag);
        let mut env = std::collections::BTreeMap::from([(
            "BANANA_ANDROID_RUN".into(),
            run.to_string_lossy().into_owned(),
        )]);
        for key in ["AF_INPUT_SOCKET", "AF_AUDIO_SOCKET"] {
            if let Some(value) = request.get(key).and_then(Value::as_str) {
                if value.len() > 100
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                {
                    return Err("Invalid Android transport socket".into());
                }
                env.insert(key.into(), value.into());
            }
        }
        let sound = request
            .get("sound")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let session = banana_runtime::start_with_env(
            game.id(),
            source,
            banana_api::ActionRequest::new("launch"),
            paths,
            files.join("opt"),
            |settings| {
                settings.players = 1;
                settings.inputs[0] = banana_input::pad::Method::KeyboardMouse;
                settings.fullscreen = false;
                settings.audio = sound;
            },
            env,
        )?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        sessions()
            .lock()
            .map_err(|_| "Session registry unavailable")?
            .insert(
                id,
                Running {
                    session: Some(session),
                    exit: None,
                },
            );
        Ok(json!({"session":id,"run":run}))
    }
}
fn poll(request: &Value) -> Result<Value, String> {
    let id = request
        .get("session")
        .and_then(Value::as_u64)
        .ok_or("Missing session")?;
    #[cfg(not(target_os = "android"))]
    {
        let _ = id;
        Err("Android session unavailable on this host".into())
    }
    #[cfg(target_os = "android")]
    {
        let mut all = sessions()
            .lock()
            .map_err(|_| "Session registry unavailable")?;
        let record = all.get_mut(&id).ok_or("Unknown session")?;
        let mut events = Vec::new();
        if let Some(session) = &mut record.session {
            while let Some(event) = session.next_event()? {
                if let banana_api::SessionEvent::Exited(code) = event {
                    record.exit = Some(code);
                }
                events.push(event);
            }
        }
        if record.exit.is_some() {
            record.session.take();
        }
        Ok(
            json!({"running":record.exit.is_none(),"exit_code":record.exit.unwrap_or(-1),"events":events}),
        )
    }
}
fn stop(request: &Value) -> Result<Value, String> {
    let id = request
        .get("session")
        .and_then(Value::as_u64)
        .ok_or("Missing session")?;
    #[cfg(target_os = "android")]
    {
        sessions()
            .lock()
            .map_err(|_| "Session registry unavailable")?
            .remove(&id);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = id;
    }
    Ok(json!({"stopped":true}))
}

/// One transport entrypoint, also exercised by host tests.
pub fn dispatch(operation: &str, payload: &str) -> String {
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        crate::initialize();
        let request: Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
        match operation {
            "catalog" => catalog(&request),
            "prepare" | "start" => start(&request),
            "poll" => poll(&request),
            "stop" => stop(&request),
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
        assert!(games.iter().all(|g| g["verified"] == false));
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
