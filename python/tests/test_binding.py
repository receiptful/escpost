import struct
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from escpost import render, render_result
from escpost.cases import Case, CaseError


REPOSITORY = Path(__file__).parents[2]
CASE_DIRECTORY = (
    REPOSITORY
    / "crates"
    / "escpost-render"
    / "tests"
    / "cases"
    / "graphics"
    / "esc-star-8dot-double-density"
)


class RenderBindingTest(unittest.TestCase):
    def test_render_returns_complete_png_sheets(self):
        data = bytes.fromhex((CASE_DIRECTORY / "input.hex").read_text())

        sheets = render(data, profile="NT-5890K")

        self.assertEqual(len(sheets), 1)
        self.assertIsInstance(sheets[0], bytes)
        self.assertEqual(_read_png_header(sheets[0]), (384, 30, 1, 0))

    def test_render_result_reports_rendering_identity(self):
        rendered = render_result(b"\n", profile="NT-5890K")

        self.assertEqual(len(rendered["sheets"]), 1)
        self.assertEqual(rendered["device_events"], [])
        self.assertEqual(rendered["warnings"], [])
        self.assertEqual(rendered["metadata"]["profile_id"], "NT-5890K")
        self.assertEqual(
            len(rendered["metadata"]["canonical_profile_sha256"]),
            64,
        )

    def test_render_result_warns_on_a_cut_without_a_cutter(self):
        # NT-5890K has no cutter: the full cut still splits the preview into two
        # receipts and reports a non-fatal warning rather than failing.
        rendered = render_result(b"\n\x1d\x56\x00\n", profile="NT-5890K")

        self.assertEqual(len(rendered["sheets"]), 2)
        self.assertEqual(len(rendered["warnings"]), 1)
        warning = rendered["warnings"][0]
        self.assertEqual(warning["type"], "uncuttable_cut")
        self.assertEqual(warning["command"], "GS V full cut")
        self.assertEqual(warning["profile"], "NT-5890K")
        self.assertIn("not physically", warning["message"])


class CaseLoaderTest(unittest.TestCase):
    def test_case_loader_accepts_a_versioned_fixture_without_a_duplicate_hash(self):
        with TemporaryDirectory() as case_directory:
            case_directory = Path(case_directory)
            (case_directory / "input.hex").write_text("1b 40")
            (case_directory / "case.toml").write_text(
                """
schema_version = 1
name = "Minimal case"
profile = "NT-5890K"
""".strip()
            )

            case = Case.load(case_directory)

        self.assertEqual(case.profile, "NT-5890K")
        self.assertEqual(case.input_bytes, b"\x1b@")

    def test_case_loader_rejects_the_retired_input_hash_field(self):
        with TemporaryDirectory() as case_directory:
            case_directory = Path(case_directory)
            (case_directory / "input.hex").write_text("1b 40")
            (case_directory / "case.toml").write_text(
                """
schema_version = 1
name = "Old case"
profile = "NT-5890K"
input_sha256 = "0fcb71d8b3b3b965f4d75d20e8d4bca56c4d13a44de0a9ac2899181d8d9b7abf"
""".strip()
            )

            with self.assertRaisesRegex(CaseError, "unknown case field 'input_sha256'"):
                Case.load(case_directory)


def _read_png_header(png):
    if png[:8] != b"\x89PNG\r\n\x1a\n":
        raise AssertionError("render result is not a PNG")

    length, chunk_type = struct.unpack(">I4s", png[8:16])
    if length != 13 or chunk_type != b"IHDR":
        raise AssertionError("PNG does not begin with an IHDR chunk")

    width, height, bit_depth, color_type, _, _, _ = struct.unpack(
        ">IIBBBBB", png[16:29]
    )
    return width, height, bit_depth, color_type


if __name__ == "__main__":
    unittest.main()
