"""Exercise the UI's staged runner without a game dump or desktop devices."""
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch


class StagedLaunch(unittest.TestCase):
    def test_staged_helpers_use_shared_project_seeds_libraries_and_save(self):
        with tempfile.TemporaryDirectory(prefix="minecraft ui ") as directory:
            project = Path(directory)
            scripts = project / "runtime/minecraft/scripts"
            scripts.mkdir(parents=True)
            for name in ("seeded", "run", "prepare", "cards", "joysticks"):
                shutil.copyfile(Path(__file__).with_name(f"minecraft-{name}.py"),
                                scripts / f"minecraft-{name}.py")
            spec = importlib.util.spec_from_file_location("staged", scripts / "minecraft-seeded.py")
            seeded = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(seeded)
            game = project / "game files"
            (game / "pmuser").mkdir(parents=True)
            (game / "gamedump.exec").write_bytes(b"test dump")
            digest = hashlib.sha256(b"test dump").hexdigest()
            seeds = {"game_sha256": digest}
            (project / "runtime/minecraft/seeds.json").write_text(json.dumps(seeds))
            libraries = project / "runtime/minecraft/lib"
            libraries.mkdir()
            original_module = seeded.module

            def load(name, path):
                loaded = original_module(name, path)
                if name == "minecraft_runner":
                    loaded.sandbox = Mock(return_value=["test-sandbox"])
                    loaded.display_options = Mock()
                elif name == "minecraft_prepare":
                    loaded.prepare = lambda data: data
                return loaded

            process = Mock()
            process.wait.return_value = 0
            args = [str(scripts / "minecraft-seeded.py"), str(game),
                    "--project-root", str(project), "--no-joysticks", "--no-cards"]
            with patch.object(sys, "argv", args), patch.object(seeded, "SOURCE_SHA256", digest), \
                    patch.object(seeded, "module", side_effect=load), \
                    patch.object(seeded, "seeded_image", return_value=b"runtime image") as image, \
                    patch.object(seeded, "wrap_gamescope", side_effect=lambda cmd, *_: cmd), \
                    patch.object(seeded.subprocess, "Popen", return_value=process) as launch:
                self.assertEqual(seeded.main(), 0)
            self.assertEqual(image.call_args.args[1], seeds)
            self.assertIn(str(libraries), launch.call_args.args[0])
            self.assertTrue((project / "runtime/minecraft-seeded/runner.log").is_file())
            self.assertEqual((game / "gamedump.exec").read_bytes(), b"test dump")


if __name__ == "__main__":
    unittest.main()
