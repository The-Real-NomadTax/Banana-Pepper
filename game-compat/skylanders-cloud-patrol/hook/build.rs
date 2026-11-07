fn main() {
    if std::env::var("TARGET").as_deref() == Ok("i686-pc-windows-msvc") {
        // The PQ Labs client API is cdecl; lld-link maps these undecorated
        // names to `_name`. Keep the original DLL's names and ordinals.
        let definition =
            std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("PQMTClient.def");
        let exports = [
            ("ConnectServer", 5),
            ("DisconnectServer", 6),
            ("GetGestureName", 7),
            ("GetServerResolution", 8),
            ("SendRequest", 9),
            ("SendThreshold", 10),
            ("SetOnGetDeviceInfo", 11),
            ("SetOnReceiveError", 12),
            ("SetOnReceiveGesture", 13),
            ("SetOnReceivePointFrame", 14),
            ("SetOnServerBreak", 15),
            ("SetRawDataResolution", 16),
            // KaboomComDll.dll (coin/ticket board), staged from the same file.
            ("CheckComm", 101),
            ("CloseLib", 102),
            ("GetCurrentCoinCount", 103),
            ("GetKeypadCount", 104),
            ("GetTicketFeederCount", 105),
            ("GetTicketFeederTicketToFeedCount", 106),
            ("InitLib", 107),
            ("ResetCoinCount", 108),
            ("ResetEmptyTicketFeeder", 109),
            ("ResetTicketFeeder", 110),
            ("SetTicketFeederTicketToFeedCount", 111),
        ];
        let mut text = String::from("LIBRARY PQMTClient\nEXPORTS\n");
        for (name, ordinal) in exports {
            text.push_str(&format!(" {name} @{ordinal}\n"));
        }
        std::fs::write(&definition, text).unwrap();
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
