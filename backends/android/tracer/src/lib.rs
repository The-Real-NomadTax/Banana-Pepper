//! A ptrace tracer that runs Android guests in their own root, the job PRoot
//! did: guest path translation over a root and binds, exec of guest ELF and
//! script programs through their guest interpreters, fake root, hard links
//! as symbolic links, and kill-on-exit. It takes PRoot's command line.
//!
//! Linux on AArch64 (Android) and x86-64 (where its tests run).
#![cfg(any(target_os = "android", target_os = "linux"))]
mod arch;
mod exec;
mod mem;
pub mod path;
mod trace;
mod view;

pub use trace::{Options, Tracer};

/// Parse PRoot's spelling: `--kill-on-exit --link2symlink -0 -r ROOT
/// -b HOST[:GUEST]… -w DIR -v LEVEL COMMAND ARGS…`.
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        cwd: "/".into(),
        ..Options::default()
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--kill-on-exit" => options.kill_on_exit = true,
            "--link2symlink" => options.link2symlink = true,
            "-0" | "--root-id" => options.fake_root = true,
            "-r" | "--rootfs" => options.root = value("-r")?.into(),
            "-w" | "--cwd" => options.cwd = value("-w")?,
            "-v" | "--verbose" => {
                options.verbose = value("-v")?.parse().map_err(|_| "-v needs a number")?
            }
            "-b" | "--bind" => {
                let bind = value("-b")?;
                let (host, guest) = match bind.split_once(':') {
                    Some((h, g)) => (h.to_owned(), g.to_owned()),
                    None => (bind.clone(), bind),
                };
                options.binds.push((host.into(), guest));
            }
            other if other.starts_with('-') => return Err(format!("Unknown option {other}")),
            _ => {
                options.argv.push(arg.clone());
                options.argv.extend(rest.cloned());
                break;
            }
        }
    }
    if options.root.as_os_str().is_empty() {
        return Err("-r ROOT is required".into());
    }
    if options.argv.is_empty() {
        return Err("No command".into());
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    #[test]
    fn proot_command_lines_parse() {
        let args: Vec<String> = "--kill-on-exit --link2symlink -0 -r /root -b /dev:/dev -b /proc -w /work /usr/bin/env -i A=1"
            .split(' ')
            .map(String::from)
            .collect();
        let o = super::parse(&args).unwrap();
        assert!(o.kill_on_exit && o.link2symlink && o.fake_root);
        assert_eq!(o.root, std::path::PathBuf::from("/root"));
        assert_eq!(o.binds.len(), 2);
        assert_eq!(o.binds[1].1, "/proc");
        assert_eq!(o.cwd, "/work");
        assert_eq!(o.argv, ["/usr/bin/env", "-i", "A=1"]);
        assert!(super::parse(&["-r".into()]).is_err());
    }
}
