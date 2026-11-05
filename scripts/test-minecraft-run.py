import importlib.util
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch
import io

spec = importlib.util.spec_from_file_location('runner', Path(__file__).with_name('minecraft-run.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

class HostSetup(unittest.TestCase):
    def test_corrupt_download_preserves_existing_library(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            (output/'packages').mkdir()
            (output/'lib').mkdir()
            library = output/'lib/libiw.so.30'
            library.write_bytes(b'existing')
            metadata = types.SimpleNamespace(stdout='wireless_tools 30 ' + '0'*64 + ' https://example.invalid/package\n')
            with patch.object(runner.subprocess, 'run', return_value=metadata) as command, patch.object(runner.urllib.request, 'urlopen', return_value=io.BytesIO(b'corrupt')):
                with self.assertRaisesRegex(RuntimeError, 'checksum/size mismatch'):
                    runner.setup_arch_wireless(output)
            self.assertEqual(library.read_bytes(), b'existing')
            self.assertEqual(command.call_count, 1, 'Corrupt packages must not be extracted')
            self.assertFalse((output/'wireless-source.json').exists())

    def test_non_https_metadata_does_not_download(self):
        metadata = types.SimpleNamespace(stdout='wireless_tools 30 ' + '0'*64 + ' http://example.invalid/package\n')
        with patch.object(runner.subprocess, 'run', return_value=metadata), patch.object(runner.urllib.request, 'urlopen') as download:
            with self.assertRaisesRegex(RuntimeError, 'Invalid wireless_tools'):
                runner.setup_arch_wireless(Path('/nonexistent'))
            download.assert_not_called()

if __name__ == '__main__': unittest.main()
