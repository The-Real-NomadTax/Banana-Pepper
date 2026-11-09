fn main() {
    if std::env::var("TARGET").as_deref() == Ok("x86_64-pc-windows-msvc") {
        // The game imports the HASP library by ordinal; the other device
        // libraries by name. Named exports sit above the HASP ordinals.
        let mut definition = String::from("LIBRARY bngrw\nEXPORTS\n");
        for (name, ordinal) in [
            ("hasp_decrypt", 2),
            ("hasp_get_size", 7),
            ("hasp_login", 13),
            ("hasp_logout", 14),
            ("hasp_read", 15),
            ("hasp_write", 17),
        ] {
            definition += &format!(" {name} @{ordinal} NONAME\n");
        }
        for (i, name) in [
            "nbamUsbFinderInitialize",
            "nbamUsbFinderGetSerialNumber",
            "nbamUsbFinderRelease",
            "BngRwAttach",
            "BngRwFin",
            "BngRwGetVersion",
            "BngRwInit",
            "BngRwIsCmdExec",
            "BngRwReqAction",
            "BngRwReqBeep",
            "BngRwReqCancel",
            "BngRwReqLed",
            "BngRwReqSendUrlTo",
            "BngRwReqWaitTouch",
        ]
        .iter()
        .enumerate()
        {
            definition += &format!(" {name} @{}\n", 32 + i);
        }
        let path = std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("bngrw.def");
        std::fs::write(&path, definition).unwrap();
        for argument in ["/NODEFAULTLIB", "/ENTRY:_DllMainCRTStartup", "/OPT:REF"] {
            println!("cargo:rustc-cdylib-link-arg={argument}");
        }
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", path.display());
    }
}
