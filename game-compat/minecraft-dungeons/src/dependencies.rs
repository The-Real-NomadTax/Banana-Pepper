use banana_api::*;
pub fn libraries() -> Dependency {
    Dependency {
        id: "minecraft".into(),
        destination: ".banana/libraries".into(),
        recipe: Some(DependencyRecipe {
            deb: &[
                "libglu1-mesa",
                "libopengl0",
                "liblapack3",
                "libblas3",
                "libgfortran5",
                "libquadmath0",
                "libiw30t64",
                "libpcsclite1",
            ],
            downloads: vec![DownloadSpec {
                url: "https://downloads.sourceforge.net/project/manualinux/Doomsday/fmodstudioapi11020linux.tar.gz",
                sha256: "53231260b694286807995714bb11011afe08361eb4873bc75cf335873e167137",
                limit: 64 * 1024 * 1024,
                members: vec![
                    (
                        "fmodstudioapi11020linux/api/lowlevel/lib/x86_64/libfmod.so.10.20",
                        "lib/libfmod.so.10",
                    ),
                    (
                        "fmodstudioapi11020linux/api/studio/lib/x86_64/libfmodstudio.so.10.20",
                        "lib/libfmodstudio.so.10",
                    ),
                    (
                        "fmodstudioapi11020linux/doc/LICENSE.TXT",
                        "lib/FMOD-LICENSE.TXT",
                    ),
                ],
            }],
        }),
    }
}
