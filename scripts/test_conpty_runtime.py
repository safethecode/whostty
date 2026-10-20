import hashlib
import io
import unittest
import zipfile

from prepare_conpty_runtime import runtime_files


def archive(files):
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as package:
        for name, data in files.items():
            package.writestr(name, data)
    return output.getvalue()


class ConptyRuntimeTest(unittest.TestCase):
    def test_checksum_mismatch_rejected_before_zip_parsing(self):
        with self.assertRaisesRegex(ValueError, 'checksum'):
            runtime_files(b'not a trusted package', '0' * 64)

    def test_host_is_required_alongside_dll(self):
        data = archive({'runtimes/win-x64/native/conpty.dll': b'dll'})
        with self.assertRaisesRegex(ValueError, 'OpenConsole'):
            runtime_files(data, hashlib.sha256(data).hexdigest())

    def test_only_matched_x64_pair_is_extracted(self):
        data = archive({
            'runtimes/win-x64/native/conpty.dll': b'dll',
            'build/native/runtimes/x64/OpenConsole.exe': b'host',
            'runtimes/win-x86/native/conpty.dll': b'wrong architecture',
            '../unwanted.txt': b'never extract',
        })
        self.assertEqual(runtime_files(data, hashlib.sha256(data).hexdigest()),
                         {'conpty.dll': b'dll', 'OpenConsole.exe': b'host'})


if __name__ == '__main__':
    unittest.main()
