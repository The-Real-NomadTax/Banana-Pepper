"""Save routing for folder and ZArchive-materialized Jurassic Park sources."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import shutil
import subprocess


spec = importlib.util.spec_from_file_location(
    "jurassic_explore", Path(__file__).with_name("jurassic-explore.py"))
jurassic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(jurassic)


class JurassicSaves(unittest.TestCase):
    @unittest.skipUnless(shutil.which("bwrap"), "Bubblewrap is required")
    def test_missing_archive_link_persists_real_sandbox_writes(self):
        with tempfile.TemporaryDirectory(prefix="jurassic missing link ") as directory:
            root = Path(directory)
            game, output = root / "game", root / "runtime"
            (game / "pmuser/adj/save").mkdir(parents=True)
            (game / "g5/dino/data").mkdir(parents=True)
            (game / "g5/dino/data/asset").write_text("read-only asset")
            (game / "pmuser/adj/save/cabinet").write_text("defaults")
            output.mkdir()
            base = ["bwrap", "--ro-bind", "/usr", "/usr"]
            for name in ["lib", "lib64", "bin"]:
                path = Path("/") / name
                if path.is_symlink():
                    base += ["--symlink", str(path.readlink()), str(path)]
                elif path.exists():
                    base += ["--ro-bind", str(path), str(path)]
            base += ["--dev", "/dev", "--ro-bind", str(game), "/pm"]
            first = base + jurassic.prepare_save_mounts(game, output) + [
                "/bin/sh", "-ec",
                'test "$(cat /pm/g5/dino/data/asset)" = "read-only asset"; '
                "echo deluxe > /pm/g5/dino/pmuser/adj/save/cabinet; "
                "echo calibrated > /pm/g5/dino/pmuser/adj/save/gun; "
                "if touch /pm/g5/dino/data/changed 2>/dev/null; then exit 1; fi",
            ]
            subprocess.run(first, check=True, capture_output=True, text=True)
            second = base + jurassic.prepare_save_mounts(game, output) + [
                "/bin/sh", "-ec",
                "test $(cat /pm/g5/dino/pmuser/adj/save/cabinet) = deluxe; "
                "test $(cat /pm/g5/dino/pmuser/adj/save/gun) = calibrated",
            ]
            subprocess.run(second, check=True, capture_output=True, text=True)
            self.assertFalse((game / "g5/dino/pmuser").exists())
            self.assertEqual((game / "pmuser/adj/save/cabinet").read_text(), "defaults")

    def test_materialized_save_directory_uses_persistent_settings_on_relaunch(self):
        with tempfile.TemporaryDirectory(prefix="jurassic saves ") as directory:
            root = Path(directory)
            game, output = root / "extracted game", root / "runtime"
            original = game / "pmuser"
            nested = game / "g5/dino/pmuser"
            for path in [original, nested, output]:
                path.mkdir(parents=True)
            for path in [original, nested]:
                (path / "adjustments.dat").write_bytes(b"cabinet defaults")

            args = jurassic.prepare_save_mounts(game, output)
            mounts = {args[i + 2]: Path(args[i + 1]) for i in range(0, len(args), 3)}
            saves = output / "pmuser"
            self.assertEqual(mounts, {
                "/pmuser": saves, "/pm/pmuser": saves,
                "/pm/g5/dino/pmuser": saves,
            })
            # The game's nested save path must reach the persistent directory.
            (mounts["/pm/g5/dino/pmuser"] / "adjustments.dat").write_bytes(b"cabinet deluxe")
            (mounts["/pm/g5/dino/pmuser"] / "calibration.dat").write_bytes(b"gun calibration")
            self.assertEqual(jurassic.prepare_save_mounts(game, output), args)
            self.assertEqual((saves / "adjustments.dat").read_bytes(), b"cabinet deluxe")
            self.assertEqual((saves / "calibration.dat").read_bytes(), b"gun calibration")
            for path in [original, nested]:
                self.assertEqual((path / "adjustments.dat").read_bytes(), b"cabinet defaults")
                self.assertFalse((path / "calibration.dat").exists())

    def test_original_guest_save_link_keeps_its_existing_alias_mounts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            game, output = root / "game", root / "runtime"
            (game / "pmuser").mkdir(parents=True)
            (game / "pmuser/adjustments.dat").write_bytes(b"defaults")
            (game / "g5/dino").mkdir(parents=True)
            (game / "g5/dino/pmuser").symlink_to("../../../pmuser")
            output.mkdir()
            self.assertEqual(jurassic.prepare_save_mounts(game, output), [
                "--bind", str(output / "pmuser"), "/pm/pmuser",
                "--bind", str(output / "pmuser"), "/pmuser",
            ])
            self.assertEqual((output / "pmuser/adjustments.dat").read_bytes(), b"defaults")


if __name__ == "__main__":
    unittest.main()
