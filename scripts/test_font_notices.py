import hashlib
import struct
import tempfile
import unittest
from pathlib import Path

from write_font_notices import FONT_NOTICES, name_metadata, render_notices


def font_with_names(records):
    storage = bytearray()
    entries = bytearray()
    for platform, language, name, text in records:
        raw = text.encode("mac_roman" if platform == 1 else "utf-16-be")
        entries.extend(struct.pack(">HHHHHH", platform, 0 if platform == 1 else 1, language, name, len(raw), len(storage)))
        storage.extend(raw)
    table = struct.pack(">HHH", 0, len(records), 6 + len(entries)) + entries + storage
    return b"\x00\x01\x00\x00" + struct.pack(">HHHH", 1, 0, 0, 0) + struct.pack(">4sIII", b"name", 0, 28, len(table)) + table


class FontNoticesTest(unittest.TestCase):
    def test_unicode_copyright_and_english_record_precedence(self):
        data = font_with_names([
            (1, 0, 0, "Legacy attribution"),
            (3, 0x409, 0, "Copyright © 2026 Example"),
            (3, 0x409, 13, "Font license terms"),
            (3, 0x409, 14, "https://example.com/license"),
        ])
        self.assertEqual(name_metadata(data), {
            0: "Copyright © 2026 Example", 13: "Font license terms", 14: "https://example.com/license",
        })
        self.assertEqual(name_metadata(font_with_names([(1, 0, 0, "Copyright © Example")])), {0: "Copyright © Example"})

    def test_truncated_tables_and_strings_are_rejected(self):
        data = font_with_names([(3, 0x409, 0, "Copyright")])
        for length in [0, 4, 12, 27, len(data) - 1]:
            with self.subTest(length=length), self.assertRaises(ValueError):
                name_metadata(data[:length])
        unsupported = bytearray(data)
        struct.pack_into(">H", unsupported, 28, 2)
        with self.assertRaises(ValueError):
            name_metadata(unsupported)
        malformed = bytearray(data)
        struct.pack_into(">H", malformed, 28 + 6 + 10, 0xFFFF)
        with self.assertRaises(ValueError):
            name_metadata(malformed)

    def test_all_fonts_retain_metadata_original_terms_and_content_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for font, notice in FONT_NOTICES.items():
                records = [] if font == "emoji-icon-font.ttf" else [(3, 0x409, 0, f"Copyright owner of {font}")]
                (directory / font).write_bytes(font_with_names(records))
                (directory / notice).write_text(f"Original full notice: {notice}\nSecond line.", encoding="utf-8")
            first = render_notices(directory, "test-version")
            self.assertEqual(first, render_notices(directory, "test-version"))
            self.assertNotIn(str(directory), first)
            for font, notice in FONT_NOTICES.items():
                self.assertIn(font, first)
                self.assertIn(hashlib.sha256((directory / font).read_bytes()).hexdigest(), first)
                self.assertIn((directory / notice).read_text(encoding="utf-8"), first)
            (directory / "extra.ttf").write_bytes(b"unknown")
            with self.assertRaises(ValueError):
                render_notices(directory, "test-version")
            (directory / "extra.ttf").unlink()
            (directory / "OFL.txt").write_text("", encoding="utf-8")
            with self.assertRaises(ValueError):
                render_notices(directory, "test-version")


if __name__ == "__main__":
    unittest.main()
