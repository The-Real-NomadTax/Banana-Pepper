//! Windows view shim (`banana_view.dll`). The Native backend stages a native
//! game's executables, adds this DLL to the staged executable's imports and
//! passes private pipes to the session view. Before any game code runs, the
//! shim hooks the Win32 file API so paths under the staged root are served
//! from the view (source, generated files and overlay); everything else
//! reaches Windows unchanged. Under Wine, or without a view, it stays inert.
#[cfg_attr(not(all(windows, target_arch = "x86")), allow(dead_code))]
mod ini;
#[cfg(all(windows, target_arch = "x86"))]
mod shim;
