fn main() {
    if std::env::var("TARGET").as_deref() == Ok("i686-pc-windows-msvc") {
        let definition =
            std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("inpout32.def");
        std::fs::write(
            &definition,
            "LIBRARY inpout32\nEXPORTS\n Out32=_Out32@8 @2\n",
        )
        .unwrap();
        for argument in [
            "/NODEFAULTLIB",
            "/ENTRY:_DllMainCRTStartup@12",
            "/SAFESEH:NO",
            "/OPT:REF",
        ] {
            println!("cargo:rustc-cdylib-link-arg={argument}");
        }
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", definition.display());
    }
}
