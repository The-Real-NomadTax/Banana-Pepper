"""Apply the small, versioned broker extension to the pinned PRoot source."""
from pathlib import Path
import sys

root = Path(sys.argv[1])
extension = Path(__file__).with_name("banana_vfs.c")
target = root / "src/extension/banana_vfs.c"
target.write_bytes(extension.read_bytes())
(root / "src/extension/banana_fd.h").write_bytes(extension.with_name("banana_fd.h").read_bytes())
p = root / "src/GNUmakefile"
s = p.read_text()
if "extension/banana_vfs.o" not in s:
    s = s.replace("OBJECTS += \\\n", "OBJECTS += extension/banana_vfs.o\n\nOBJECTS += \\\n", 1)
p.write_text(s)
p = root / "src/cli/cli.c"
s = p.read_text()
needle = "\t/* Pre-configure the first tracee.  */"
if "banana_vfs_callback" not in s:
    s = s.replace(needle, """\t/* Initialize before parsing -w: its directory may be virtual. */
    if (getenv("BANANA_VFS_SOCKET") != NULL) {
        extern int banana_vfs_callback(Extension *, ExtensionEvent, intptr_t, intptr_t);
        if (initialize_extension(tracee, banana_vfs_callback, NULL) < 0) goto error;
    }
""" + needle, 1)
p.write_text(s)

p = root / "src/cli/cli.c"
s = p.read_text()
needle = "status = canonicalize(tracee, path2, true, path, 0);"
if "banana_vfs_guest_path" not in s:
    s = s.replace(needle, """extern int banana_vfs_guest_path(Tracee *, char *, const char *, int);
    status = banana_vfs_guest_path(tracee, path, path2, 1);
    if (status == 0) status = canonicalize(tracee, path2, true, path, 0);
    else if (status > 0) status = 0;""", 1)
p.write_text(s)
p = root / "src/path/path.c"
s = p.read_text()
needle = "return canonicalize(tracee, joined, deref_final, guest_path, 0);"
if "banana_vfs_guest_path" not in s:
    s = s.replace(needle, """extern int banana_vfs_guest_path(Tracee *, char *, const char *, int);
    status = banana_vfs_guest_path(tracee, guest_path, joined, 0);
    if (status != 0) return status > 0 ? 0 : status;
    return canonicalize(tracee, joined, deref_final, guest_path, 0);""", 1)
p.write_text(s)

# New NDK clang versions require the declarations used by this upstream unit.
p = root / "src/extension/ashmem_memfd/ashmem_memfd.c"
s = p.read_text()
if "#include <string.h>" not in s:
    s = "#include <string.h>\n" + s
p.write_text(s)

# Android sessions own every traced descendant, including Wine servers that
# create another process group. The kernel reaps them if their tracer dies.
p = root / "src/tracee/event.c"
s = p.read_text()
s = s.replace("PTRACE_O_TRACEEXIT);", "PTRACE_O_TRACEEXIT | PTRACE_O_EXITKILL);")
p.write_text(s)
