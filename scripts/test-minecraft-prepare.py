"""Validate the import repair against user-supplied files; no game fixture is shipped."""
import importlib.util
from pathlib import Path
import struct
import sys
import unittest

spec = importlib.util.spec_from_file_location("minecraft_prepare", Path(__file__).with_name("minecraft-prepare.py"))
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)
source_path = Path(sys.argv.pop(1)) if len(sys.argv) > 1 else None


class ImportRepair(unittest.TestCase):
    def test_unknown_build_is_rejected(self):
        with self.assertRaises(ValueError):
            prepare.prepare(b"unrecognized executable")

    @unittest.skipIf(source_path is None, "Pass the local gamedump.exec to validate reconstruction")
    def test_repair_preserves_game_code_and_restores_linker_metadata(self):
        source = source_path.read_bytes()
        result = prepare.prepare(source)
        self.assertEqual(source[24:32], result[24:32])  # original entry
        self.assertEqual(source[0xBF30:0xD73300], result[0xBF30:0xD73300])
        changed = {offset for offset, (a, b) in enumerate(zip(source, result)) if a != b}
        allowed = set(range(64 + 4 * 56, 64 + 5 * 56))
        allowed.update(range(0x12C77A8, 0x12C77A8 + 0x290))
        self.assertTrue(changed <= allowed, "Only ELF metadata may change within the supplied snapshot")
        load = struct.unpack_from('<IIQQQQQQ', result, 64 + 4 * 56)
        self.assertEqual(load[:2], (1, 6))
        self.assertEqual(load[3], 0x3000000)
        self.assertEqual(load[5], len(result) - load[2])
        self.assertEqual(struct.unpack_from('<I', source, 0x30487)[0], struct.unpack_from('<I', result, 0x30487)[0])


if __name__ == '__main__':
    unittest.main()
