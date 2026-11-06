#!/usr/bin/env python3
"""Minecraft Linux guest runner. Invoked over stdin by the Windows frontend.

Assets remain read-only. Only runtime/minecraft is writable in the guest.
No cabinet boot scripts, privileged host setup, or third-party loaders are used.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import importlib.util
import html
import re
import tarfile
import urllib.request
import urllib.parse

EXPECTED = "663b147927e7b9d60cef7bd41cf49d3fd64058619880391083ea1ba872d6be3b"
PACKAGES = ["libglu1-mesa", "libopengl0", "liblapack3", "libblas3", "libgfortran5", "libquadmath0", "libiw30t64", "libpcsclite1"]
FMOD_URL = "https://downloads.sourceforge.net/project/manualinux/Doomsday/fmodstudioapi11020linux.tar.gz"
FMOD_SHA256 = "53231260b694286807995714bb11011afe08361eb4873bc75cf335873e167137"


def setup_fmod(output):
    """Cache the archived SDK locally; never bundle its binaries with Banana pepper."""
    archive = output / "packages/fmodstudioapi11020linux.tar.gz"
    if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest() != FMOD_SHA256:
        url = FMOD_URL
        temporary = archive.with_suffix(".download")
        for attempt in range(3):
            request = urllib.request.Request(url, headers={"User-Agent": "BananaPepper/0.1"})
            with urllib.request.urlopen(request, timeout=45) as response:
                first = response.read(1024)
                if first.startswith(b"\x1f\x8b"):
                    length = len(first)
                    with temporary.open("wb") as downloaded:
                        downloaded.write(first)
                        while chunk := response.read(1024 * 1024):
                            length += len(chunk)
                            if length > 64 * 1024 * 1024:
                                raise RuntimeError("FMOD archive exceeds the expected size limit")
                            downloaded.write(chunk)
                    if hashlib.sha256(temporary.read_bytes()).hexdigest() != FMOD_SHA256:
                        raise RuntimeError("FMOD archive checksum mismatch; no libraries installed")
                    temporary.replace(archive)
                    break
                page = (first + response.read(512 * 1024)).decode("utf-8", errors="replace")
                link = re.search(re.escape(FMOD_URL) + r'\?[^"<>\s]+', page)
                if not link:
                    raise RuntimeError("SDK mirror did not return the expected archive or download link")
                url = html.unescape(link.group(0))
        else:
            raise RuntimeError("SDK mirror did not complete the download")
    libraries = output / "lib"
    libraries.mkdir(exist_ok=True)
    records = {}
    with tarfile.open(archive) as sdk:
        for name, member in {
            "libfmod.so.10": "api/lowlevel/lib/x86_64/libfmod.so.10.20",
            "libfmodstudio.so.10": "api/studio/lib/x86_64/libfmodstudio.so.10.20",
            "FMOD-LICENSE.TXT": "doc/LICENSE.TXT",
        }.items():
            stream = sdk.extractfile("fmodstudioapi11020linux/" + member)
            if stream is None:
                raise RuntimeError("SDK member is not a regular file")
            content = stream.read()
            if name.endswith(".10") and content[:5] != b"\x7fELF\x02":
                raise RuntimeError("SDK library is not ELF64")
            (libraries / name).write_bytes(content)
            records[name] = hashlib.sha256(content).hexdigest()
    (output / "fmod-source.json").write_text(json.dumps({
        "url": FMOD_URL, "archive_sha256": FMOD_SHA256, "files": records,
        "note": "Local SDK cache only. See lib/FMOD-LICENSE.TXT. Not included in Banana pepper source distributions."
    }, indent=2))
    print("FMOD 1.10.20 x86-64 libraries staged and checksummed.", flush=True)


def setup_arch_wireless(output):
    """Stage only libiw from the host's package metadata; never install it."""
    metadata = subprocess.run(
        ["pacman", "-Sp", "--print-format", "%n %v %h %l", "wireless_tools"],
        capture_output=True, text=True, check=True, timeout=30).stdout.strip().splitlines()
    if len(metadata) != 1:
        raise RuntimeError("Expected one wireless_tools package in the host package database")
    name, version, digest, url = metadata[0].split()
    if name != "wireless_tools" or not re.fullmatch(r"[0-9a-f]{64}", digest) or urllib.parse.urlparse(url).scheme != "https":
        raise RuntimeError("Invalid wireless_tools package metadata")
    archive = output / "packages" / ("wireless_tools-" + digest + ".pkg.tar.zst")
    if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest() != digest:
        with urllib.request.urlopen(url, timeout=45) as response:
            content = response.read(16 * 1024 * 1024 + 1)
        if len(content) > 16 * 1024 * 1024 or hashlib.sha256(content).hexdigest() != digest:
            raise RuntimeError("wireless_tools checksum/size mismatch; no library installed")
        temporary = archive.with_suffix(".download")
        temporary.write_bytes(content)
        temporary.replace(archive)
    # Extract one explicitly named member to stdout; no package paths/scripts
    # are installed or allowed to write into the host filesystem.
    library = subprocess.run(["tar", "-xOf", str(archive), "usr/lib/libiw.so.30"],
                             capture_output=True, check=True, timeout=30).stdout
    if library[:5] != b"\x7fELF\x02":
        raise RuntimeError("wireless_tools library is not ELF64")
    destination = output / "lib/libiw.so.30"
    temporary = destination.with_suffix(".preparing")
    temporary.write_bytes(library)
    temporary.replace(destination)
    (output / "wireless-source.json").write_text(json.dumps({
        "package": name, "version": version, "url": url, "archive_sha256": digest,
        "library_sha256": hashlib.sha256(library).hexdigest(),
    }, indent=2) + "\n")
    print("Host wireless_tools libiw staged and checksum-verified.", flush=True)


def setup(output):
    """Extract libraries locally, without sudo or system package changes."""
    cache = output / "packages"
    cache.mkdir(exist_ok=True)
    libraries = output / "libraries"
    libraries.mkdir(exist_ok=True)
    (output / "lib").mkdir(exist_ok=True)
    if shutil.which("pacman"):
        setup_arch_wireless(output)
        setup_fmod(output)
        print("Arch/CachyOS compatibility libraries staged. Other dependencies use the host's existing libraries.")
        return
    if not shutil.which("apt-get") or not shutil.which("dpkg-deb"):
        raise RuntimeError("Automatic library setup requires apt/dpkg or pacman; libraries can also be supplied in runtime/minecraft/lib")
    for package in PACKAGES:
        result = subprocess.run(["apt-get", "download", package], cwd=cache,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=120)
        print(result.stdout, end="", flush=True)
        if result.returncode:
            raise RuntimeError(f"Could not download {package}; refresh your Ubuntu package index and retry.")
    for archive in sorted(cache.glob("*.deb")):
        subprocess.run(["dpkg-deb", "--extract", str(archive), str(libraries)], check=True, timeout=60)
    setup_fmod(output)
    print("Ubuntu and FMOD libraries staged locally. Run Minecraft diagnostics next.")


def sandbox(game, output):
    if not shutil.which("bwrap"):
        raise RuntimeError("Bubblewrap is required for the isolated Linux runtime")
    command = ["bwrap", "--unshare-all", "--die-with-parent", "--new-session", "--clearenv",
               "--ro-bind", "/usr", "/usr", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"]
    for name in ("bin", "sbin", "lib", "lib64"):
        path = Path("/") / name
        if path.is_symlink():
            command += ["--symlink", os.readlink(path), str(path)]
        elif path.exists():
            command += ["--ro-bind", str(path), str(path)]
    for path in ("/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/fonts", "/etc/alternatives", "/etc/localtime"):
        if Path(path).exists():
            command += ["--ro-bind", path, path]
    command += ["--bind", str(output), "/work", "--dir", "/pm/g7/minecraft"]
    for name in ("data", "eFileMaps", "utils", "minecraft.hvc"):
        source = game / name
        if source.exists():
            command += ["--ro-bind", str(source), f"/pm/g7/minecraft/{name}"]
    command += ["--symlink", "/work/pmuser", "/pm/g7/minecraft/pmuser",
                "--ro-bind", str(output / "game"), "/pm/g7/minecraft/game",
                "--chdir", "/pm/g7/minecraft"]
    paths = ["/work/lib", "/work/libraries/usr/lib/x86_64-linux-gnu",
             "/work/libraries/usr/lib/x86_64-linux-gnu/lapack", "/work/libraries/usr/lib/x86_64-linux-gnu/blas",
             "/work/libraries/lib/x86_64-linux-gnu"]
    for key, value in {"PATH": "/usr/bin:/bin", "HOME": "/work/home", "LANG": "C.UTF-8",
                       "LD_LIBRARY_PATH": ":".join(paths)}.items():
        command += ["--setenv", key, value]
    return command


def display_options(command):
    display = os.environ.get("DISPLAY", "")
    if not display:
        raise RuntimeError("No graphical display found. Windows requires WSL2 with WSLg.")
    x11 = Path("/tmp/.X11-unix")
    if not x11.exists():
        raise RuntimeError("X11 socket directory is missing; start a desktop X11/XWayland session (WSLg on Windows).")
    command += ["--ro-bind", str(x11.resolve()), "/tmp/.X11-unix", "--setenv", "DISPLAY", display]
    for path in ("/mnt/wslg", "/usr/lib/wsl"):
        if Path(path).exists():
            command += ["--ro-bind", path, path]
    for device in ("/dev/dxg", "/dev/dri"):
        if Path(device).exists():
            command += ["--dev-bind", device, device]
    for key in ("PULSE_SERVER", "XDG_RUNTIME_DIR", "XAUTHORITY"):
        value = os.environ.get(key)
        if value:
            if key == "XAUTHORITY" and Path(value).is_file():
                command += ["--ro-bind", value, value]
            command += ["--setenv", key, value]


def probe_license(command, output):
    """Capture the actual supported-build API status before its logger faults.

    This reads a status field only. It does not modify login results, decrypt
    seed tables, or skip cabinet security initialization.
    """
    if not shutil.which("gdb"):
        return {"status": "diagnostic_unavailable", "reason": "gdb is required in Ubuntu for protected-build startup diagnostics"}
    commands = """set pagination off
set confirm off
set print thread-events off
set disable-randomization off
set startup-with-shell off
break *0x559470
run
if $pc == 0x559470
printf "ARCADE_HASP_STATUS=%u\\n", *(unsigned int*)0x17bad58
printf "ARCADE_WRAPPER_STATUS=%u\\n", $esi
end
"""
    (output / "license-probe.gdb").write_text(commands)
    try:
        result = subprocess.run(command + ["gdb", "--batch", "--nx", "--nh", "-x", "/work/license-probe.gdb",
            "--args", "/pm/g7/minecraft/game", "-f1280x720", "-n"], capture_output=True, text=True, timeout=30)
    except subprocess.TimeoutExpired:
        return {"status": "diagnostic_timeout", "reason": "No licensing status captured within 30 seconds"}
    trace = result.stdout + result.stderr
    (output / "license-probe.log").write_text(trace)
    found = re.search(r"^ARCADE_HASP_STATUS=(\d+)$", trace, re.MULTILINE)
    if not found:
        return {"status": "diagnostic_failed", "reason": "The game did not reach the expected licensing diagnostic point", "gdb_exit_code": result.returncode}
    code = int(found.group(1))
    wrapper = re.search(r"^ARCADE_WRAPPER_STATUS=(\d+)$", trace, re.MULTILINE)
    return {"status": "key_not_found" if code == 7 else "api_error" if code else "no_api_error_at_diagnostic_point",
            "hasp_status": code, "wrapper_status": int(wrapper.group(1)) if wrapper else None,
            "instructions_modified": False}


def run(game, output, mode):
    if mode == "setup":
        setup(output)
        return 0
    executable = game / "gamedump.exec"
    digest = hashlib.sha256(executable.read_bytes()).hexdigest()
    if digest != EXPECTED:
        raise RuntimeError(f"Unrecognized gamedump.exec SHA256 {digest}; refusing execution.")
    module = importlib.util.spec_from_file_location("minecraft_prepare", output / "minecraft_prepare.py")
    prepare = importlib.util.module_from_spec(module)
    module.loader.exec_module(prepare)
    (output / "game").write_bytes(prepare.prepare(executable.read_bytes()))
    (output / "game").chmod(0o755)
    (output / "lib").mkdir(exist_ok=True)
    (output / "home").mkdir(exist_ok=True)
    saves = output / "pmuser"
    if not saves.exists():
        if (game / "pmuser").exists():
            shutil.copytree(game / "pmuser", saves)
        else:
            saves.mkdir()
    command = sandbox(game, output)
    loader = "/lib64/ld-linux-x86-64.so.2"
    checked = subprocess.run(command + [loader, "--list", "/pm/g7/minecraft/game"],
                             capture_output=True, text=True, timeout=30)
    dependencies = checked.stdout + checked.stderr
    (output / "dependencies.log").write_text(dependencies)
    report = {"sha256": digest, "mode": mode, "dependency_exit_code": checked.returncode,
              "runtime_sha256": hashlib.sha256((output / "game").read_bytes()).hexdigest(),
              "reconstructed_imports": 659, "gameplay_verified": False, "dependencies": dependencies}
    if checked.returncode:
        # ldd reports all unresolved SONAMEs rather than only the first. It runs
        # inside the same mount, process and network isolation as the game.
        inventory = subprocess.run(command + ["/usr/bin/ldd", "/pm/g7/minecraft/game"],
                                   capture_output=True, text=True, timeout=30)
        dependencies = inventory.stdout + inventory.stderr + dependencies
        (output / "dependencies.log").write_text(dependencies)
        report["dependencies"] = dependencies
        missing = sorted({line.split()[0] for line in dependencies.splitlines() if "=> not found" in line})
        report["missing_libraries"] = missing
        print("Minecraft cannot start: missing or incompatible Linux libraries.\n" + dependencies)
        if any("fmod" in item for item in missing):
            print(f"Supply matching FMOD 1.10 x86-64 libraries as {output}/lib/libfmod.so.10 and libfmodstudio.so.10.")
        (output / "report.json").write_text(json.dumps(report, indent=2))
        return 1
    protected = []
    for name in ("programs.zip", "scenes.zip"):
        path = game / "data/prod/archives" / name
        if path.is_file():
            with path.open("rb") as stream:
                signature = stream.read(4)
            if signature not in (b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"):
                protected.append(str(path.relative_to(game)))
    report["missing_libraries"] = []
    report["protected_archives"] = protected
    display_options(command)
    if protected:
        print("All Linux shared libraries resolved, including FMOD. Reconstructed 659 snapshot imports.")
        licensing = probe_license(command, output)
        report["licensing"] = licensing
        if licensing["status"] != "no_api_error_at_diagnostic_point":
            report["status"] = "hasp_key_not_found" if licensing.get("hasp_status") == 7 else "licensing_startup_failed"
            (output / "report.json").write_text(json.dumps(report, indent=2))
            if licensing.get("hasp_status") == 7:
                print("Minecraft's actual licensing call returned HASP status 7: protection key not found.")
                print("The game needs that key to initialize the seed tables used to read its protected assets.")
            else:
                print("Licensing diagnostic:", json.dumps(licensing))
            print(f"Trace: {output / 'license-probe.log'}\nReport: {output / 'report.json'}")
            return 1
    log_path = output / "runner.log"
    print(f"Dependencies resolved. Starting isolated Minecraft guest. Log: {log_path}", flush=True)
    with log_path.open("w") as log:
        process = subprocess.Popen(command + ["/pm/g7/minecraft/game", "-f1280x720", "-n"],
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=25 if mode == "probe" else None)
            report["exit_code"] = code
            print(f"Minecraft exited with code {code}. Gameplay remains unverified.")
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            report["probe_timeout"] = True
            print("Guest stayed running for 25 seconds. This confirms process survival only; gameplay remains unverified.")
    (output / "report.json").write_text(json.dumps(report, indent=2))
    print(log_path.read_text(errors="replace")[-10000:])
    return 0 if report.get("probe_timeout") or report.get("exit_code") == 0 else 1


def main():
    game, output, mode = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    if mode not in ("setup", "probe", "launch"):
        raise RuntimeError("Unknown operation")
    if game == output or game in output.parents or output in game.parents:
        raise RuntimeError("Runtime directory must be separate from the original game directory")
    output.mkdir(parents=True, exist_ok=True)
    with (output / "session.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError("Another Minecraft operation is already running") from None
        return run(game, output, mode)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Minecraft runner: {error}", file=sys.stderr)
        sys.exit(1)
