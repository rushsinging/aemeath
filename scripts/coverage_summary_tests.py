import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("coverage_summary.py")
SPEC = importlib.util.spec_from_file_location("coverage_summary", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class CoverageSummaryTests(unittest.TestCase):
    def test_format_row_prints_all_three_metrics(self):
        values = {
            "regions": [5, 10],
            "functions": [3, 4],
            "lines": [8, 10],
        }

        output = MODULE.format_row(
            "runtime", values, ("regions", "functions", "lines")
        )

        self.assertIn("5/10 (50.00%)", output)
        self.assertIn("3/4 (75.00%)", output)
        self.assertIn("8/10 (80.00%)", output)

    def test_main_marks_package_without_countable_items_as_na(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report = root / "report.json"
            report.write_text(json.dumps({"data": [{"files": []}]}))

            original = MODULE.subprocess.check_output
            MODULE.subprocess.check_output = lambda *args, **kwargs: json.dumps(
                {
                    "packages": [
                        {
                            "name": "audit",
                            "manifest_path": str(root / "audit" / "Cargo.toml"),
                        }
                    ]
                }
            )
            try:
                from contextlib import redirect_stdout
                from io import StringIO

                stream = StringIO()
                with redirect_stdout(stream):
                    original_argv = MODULE.sys.argv
                    MODULE.sys.argv = ["coverage_summary.py", str(report), str(root)]
                    try:
                        result = MODULE.main()
                    finally:
                        MODULE.sys.argv = original_argv
            finally:
                MODULE.subprocess.check_output = original

            self.assertEqual(result, 0)
            self.assertIn("audit", stream.getvalue())
            self.assertIn("n/a", stream.getvalue())


if __name__ == "__main__":
    unittest.main()
