fn main() {
    if std::env::var("TARGET").as_deref() == Ok("i686-pc-windows-msvc") {
        for argument in [
            "/NODEFAULTLIB",
            "/ENTRY:_DllMainCRTStartup@12",
            "/EXPORT:DirectInput8Create=_DirectInput8Create@20",
            "/SAFESEH:NO",
            "/OPT:REF",
        ] {
            println!("cargo:rustc-cdylib-link-arg={argument}");
        }
    }
}
