use super::*;
static mut STEP: usize = 0;
static mut HUNT_TICK: u128 = u128::MAX;
static mut MOUSE_RELEASE: Option<(u128, i32, i32, i32)> = None;
unsafe fn capture(name: &str) {
    if let Some(host) = HOST.get() {
        if let Ok(name) = CString::new(name) {
            (host.capture)(name.as_ptr());
        }
    }
}
pub unsafe extern "C" fn probe(elapsed: u64) {
    let elapsed = elapsed as u128;
    if std::env::var_os("ARCADE_NATIVE_PROBE").is_none() {
        return;
    }
    if let Some((time, button, x, y)) = MOUSE_RELEASE {
        if elapsed >= time {
            jamma::mouse(button, false, x, y);
            MOUSE_RELEASE = None;
        }
    }
    if std::env::var_os("ARCADE_NATIVE_FIRE_PROBE").is_some() && (48000..50000).contains(&elapsed) {
        let tick = (elapsed - 48000) / 200;
        if tick != HUNT_TICK {
            HUNT_TICK = tick;
            let x = 80 + ((tick / 2 * 120) % 480) as i32;
            let y = 260;
            let button = if tick % 2 == 0 { 2 } else { 0 };
            probe_mouse(elapsed, button, x, y);
        }
    }
    // Repeat the measured Linux progression sequence through the same cabinet
    // adapter. No OS input injection, no automatic inputs in ordinary sessions.
    let steps: &[(u128, u8, i32, i32, &str)] = &[
        (2000, b't', 0, 0, "calibration.png"),
        (2200, 0, 0, 0, ""),
        (4000, b'1', 0, 0, "tracking.png"),
        (4200, 0, 0, 0, ""),
        (6000, b'5', 0, 0, "attract.png"),
        (6300, 0, 0, 0, ""),
        (7000, b'5', 0, 0, ""),
        (7300, 0, 0, 0, ""),
        (8000, b'5', 0, 0, ""),
        (8300, 0, 0, 0, ""),
        (9000, b'5', 0, 0, ""),
        (9300, 0, 0, 0, "coins.png"),
        (11000, b'1', 0, 0, "before-start.png"),
        (11300, 0, 0, 0, ""),
        (14000, 1, 160, 220, "game-select.png"),
        (17000, 1, 160, 220, "mode-select.png"),
        (20000, 1, 160, 140, "adventure-select.png"),
        (23000, 1, 160, 180, "adventure-intro.png"),
        (26000, 1, 160, 180, "records.png"),
        (29000, 1, 160, 180, "instructions.png"),
        (35000, 1, 320, 240, "instructions2.png"),
        (39000, 1, 120, 150, "site-select.png"),
        (45000, 1, 120, 150, "site-select2.png"),
        (50000, 1, 350, 240, "hunt.png"),
        (51000, 2, 350, 240, "after-shot.png"),
        (52000, 1, 300, 200, "after-pump.png"),
        (53000, 2, 300, 200, ""),
        (54000, 1, 200, 300, ""),
        (58000, 0, 0, 0, "final.png"),
        (62000, 0, 0, 0, "shot-result.png"),
        (70000, 0, 0, 0, "hunt-end.png"),
        (72000, 1, 320, 400, "initials.png"),
        (80000, 0, 0, 0, "results.png"),
        (90000, 0, 0, 0, "site-results.png"),
    ];
    if STEP >= steps.len() {
        return;
    }
    let (time, key, x, y, name) = steps[STEP];
    if elapsed < time {
        return;
    }
    if !name.is_empty() {
        capture(name);
        eprintln!("[water] probe checkpoint {name}");
    }
    if std::env::var_os("ARCADE_NATIVE_FIRE_PROBE").is_some() && (50000..60000).contains(&elapsed) {
        // Observe the first hunt's outcome without later shots hitting a doe.
    } else if key == 1 || key == 2 {
        probe_mouse(elapsed, if key == 1 { 0 } else { 2 }, x, y);
    } else {
        jamma::key(b't', false);
        jamma::key(b'1', false);
        jamma::key(b'5', false);
        if key != 0 {
            jamma::key(key, true);
        }
    }
    STEP += 1;
}
unsafe fn probe_mouse(elapsed: u128, button: i32, x: i32, y: i32) {
    let previous = MOUSE_RELEASE;
    MOUSE_RELEASE = None;
    if let Some((_, old, x, y)) = previous {
        jamma::mouse(old, false, x, y);
    }
    jamma::mouse(button, true, x, y);
    MOUSE_RELEASE = Some((elapsed + 100, button, x, y));
}
