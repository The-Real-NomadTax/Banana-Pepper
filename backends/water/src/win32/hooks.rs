use super::display_config;
pub unsafe fn players() -> usize {
    unsafe { (display_config().players)() }
}
pub unsafe fn aim_position(player: usize) -> (i32, i32) {
    let (mut x, mut y) = (0, 0);
    unsafe { (display_config().aim_position)(player, &mut x, &mut y) };
    (x, y)
}
pub unsafe fn release() {
    unsafe { (display_config().release)() }
}
pub unsafe fn key(k: u8, down: bool) {
    unsafe { (display_config().key)(k, down) }
}
pub unsafe fn aim(x: i32, y: i32, shoot: bool) {
    unsafe { (display_config().aim)(x, y, shoot) }
}
pub unsafe fn mouse(b: i32, down: bool, x: i32, y: i32) {
    unsafe { (display_config().mouse)(b, down, x, y) }
}
pub unsafe fn controller_frame(focused: bool) {
    unsafe { (display_config().controller_frame)(focused) }
}
pub unsafe fn register_raw(window: windows_sys::Win32::Foundation::HWND) {
    unsafe { (display_config().register_raw)(window as usize) }
}
pub unsafe fn raw_input(l: isize) {
    unsafe { (display_config().raw_input)(l) }
}
