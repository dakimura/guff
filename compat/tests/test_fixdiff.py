"""Unit tests for compat/fix/fixdiff.py (the --fix tier's diff engine).

The engine's whole job is to be a *byte-stable* description of what a tool wrote,
so the tests that matter are the ones about bytes: a missing final newline, a
file that only a formatter would touch, and the difference between "no expected
file" and "an expected file recording nothing".
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT / "compat" / "fix"))

import fixdiff  # noqa: E402


def tree(root: Path, files: dict[str, str]) -> Path:
    for rel, text in files.items():
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    return root


class TreeDiffTest(unittest.TestCase):
    def diff(self, before: dict[str, str], after: dict[str, str]) -> str:
        with tempfile.TemporaryDirectory() as tmp:
            b = tree(Path(tmp) / "b", before)
            a = tree(Path(tmp) / "a", after)
            return fixdiff.tree_diff(b, a)

    def test_identical_trees_diff_to_nothing(self):
        self.assertEqual(self.diff({"x.go": "package p\n"}, {"x.go": "package p\n"}), "")

    def test_matches_diff_u_byte_for_byte(self):
        """The recording is reviewed as a diff, so it has to *be* one."""
        before = {"a.go": "package p\n\nvar A = 1\nvar B = 2\nvar C = 3\n"}
        after = {"a.go": "package p\n\nvar A = 1\nvar B = 99\nvar C = 3\n"}
        with tempfile.TemporaryDirectory() as tmp:
            b = tree(Path(tmp) / "b", before)
            a = tree(Path(tmp) / "a", after)
            mine = fixdiff.tree_diff(b, a)
            real = subprocess.run(
                ["diff", "-u", str(b / "a.go"), str(a / "a.go")],
                capture_output=True,
                text=True,
            ).stdout
            # diff(1) stamps its headers with mtimes; everything after them is
            # the part that has to agree.
            self.assertEqual(mine.split("\n")[2:], real.split("\n")[2:])

    def test_missing_final_newline_is_spelled_out(self):
        """`x := 1` with no trailing newline is a real fixture shape.

        Rendered naively the next diff line runs onto the same line and the
        recording stops round-tripping.
        """
        out = self.diff({"a.go": "package p"}, {"a.go": "package p\n"})
        self.assertIn(fixdiff.NO_NEWLINE, out)
        self.assertIn("-package p", out)
        self.assertIn("+package p", out)

    def test_added_and_removed_files_use_dev_null(self):
        added = self.diff({}, {"new.go": "package p\n"})
        self.assertIn("--- /dev/null", added)
        self.assertIn("+++ b/new.go", added)
        removed = self.diff({"old.go": "package p\n"}, {})
        self.assertIn("--- a/old.go", removed)
        self.assertIn("+++ /dev/null", removed)

    def test_files_are_ordered_by_path(self):
        out = self.diff(
            {"b.go": "package p\n", "a.go": "package p\n"},
            {"b.go": "package q\n", "a.go": "package q\n"},
        )
        self.assertLess(out.index("a/a.go"), out.index("a/b.go"))

    def test_non_utf8_file_is_reported_not_crashed(self):
        with tempfile.TemporaryDirectory() as tmp:
            b, a = Path(tmp) / "b", Path(tmp) / "a"
            b.mkdir()
            a.mkdir()
            (b / "x.bin").write_bytes(b"\xff\xfe\x00")
            (a / "x.bin").write_bytes(b"\xff\xfe\x01")
            self.assertIn("Binary files", fixdiff.tree_diff(b, a))


class RoundTripTest(unittest.TestCase):
    def test_header_is_stripped_and_body_survives(self):
        body = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-package p\n+package q\n"
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "x.diff"
            p.write_text(fixdiff.render_expected("x", body, "2.12.2"), encoding="utf-8")
            self.assertEqual(fixdiff.parse_expected(p), body)

    def test_a_hash_inside_the_diff_is_content_not_a_comment(self):
        """`#` starts a build-constraint line and a shell line in testdata.

        Filtering comments line-by-line — the way the golden parser can afford
        to — would eat them out of the middle of a hunk.
        """
        body = "--- a/x.sh\n+++ b/x.sh\n@@ -1,2 +1,2 @@\n #!/bin/sh\n-echo a\n+echo b\n"
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "x.diff"
            p.write_text(fixdiff.render_expected("x", body, "2.12.2"), encoding="utf-8")
            self.assertEqual(fixdiff.parse_expected(p), body)

    def test_blank_context_line_survives(self):
        """An empty source line is a context line spelled `" "`, not `""`."""
        body = "--- a/x.go\n+++ b/x.go\n@@ -1,3 +1,3 @@\n package p\n \n-var A = 1\n+var A = 2\n"
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "x.diff"
            p.write_text(fixdiff.render_expected("x", body, "2.12.2"), encoding="utf-8")
            self.assertEqual(fixdiff.parse_expected(p), body)


class ConfirmTest(unittest.TestCase):
    def test_two_agreeing_runs_confirm(self):
        self.assertEqual(fixdiff.confirm(["a", "b", "a"], 2), "a")

    def test_disagreeing_runs_do_not_confirm(self):
        self.assertIsNone(fixdiff.confirm(["a", "b", "c"], 2))

    def test_one_confirmation_takes_the_first_run(self):
        self.assertEqual(fixdiff.confirm(["a", "b"], 1), "a")


class CliTest(unittest.TestCase):
    def run_cli(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(ROOT / "compat" / "fix" / "fixdiff.py"), *args],
            capture_output=True,
            text=True,
        )

    def test_absent_expectation_means_strictly_no_changes(self):
        """The convention this tier shares with compat/health.py's baseline."""
        with tempfile.TemporaryDirectory() as tmp:
            empty = Path(tmp) / "actual.diff"
            empty.write_text("", encoding="utf-8")
            ok = self.run_cli(
                "check", "--case", "x",
                "--actual", str(empty),
                "--expected", str(Path(tmp) / "nope.diff"),
            )
            self.assertEqual(ok.returncode, 0, ok.stderr)

            changed = Path(tmp) / "changed.diff"
            changed.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n", encoding="utf-8"
            )
            bad = self.run_cli(
                "check", "--case", "x",
                "--actual", str(changed),
                "--expected", str(Path(tmp) / "nope.diff"),
            )
            self.assertEqual(bad.returncode, 1)
            self.assertIn("differs", bad.stderr)

    def test_pending_refuses_to_hold_a_case_upstream_never_touches(self):
        """A missing fixer is a deferral; an invented edit is not."""
        with tempfile.TemporaryDirectory() as tmp:
            actual = Path(tmp) / "actual.diff"
            actual.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n", encoding="utf-8"
            )
            r = self.run_cli(
                "pending", "--case", "x",
                "--actual", str(actual),
                "--expected", str(Path(tmp) / "absent.diff"),
                "-o", str(Path(tmp) / "pending" / "x.diff"),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("REFUSING to hold", r.stderr)
            self.assertFalse((Path(tmp) / "pending" / "x.diff").exists())

    def test_pending_holds_a_case_where_upstream_fixes_and_guff_does_not(self):
        with tempfile.TemporaryDirectory() as tmp:
            actual = Path(tmp) / "actual.diff"
            actual.write_text("", encoding="utf-8")
            expected = Path(tmp) / "expected.diff"
            expected.write_text(
                fixdiff.render_expected(
                    "x", "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n", "2.12.2"
                ),
                encoding="utf-8",
            )
            out = Path(tmp) / "pending" / "x.diff"
            r = self.run_cli(
                "pending", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "-o", str(out),
            )
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertTrue(out.exists())
            self.assertEqual(fixdiff.parse_expected(out), "")

    def test_write_removes_a_stale_expectation_when_upstream_stops_fixing(self):
        """Otherwise a linter that loses its fixer keeps being gated on the old one."""
        with tempfile.TemporaryDirectory() as tmp:
            run = Path(tmp) / "run.diff"
            run.write_text("", encoding="utf-8")
            out = Path(tmp) / "expected" / "x.diff"
            out.parent.mkdir()
            out.write_text("stale\n", encoding="utf-8")
            r = self.run_cli(
                "write", "--case", "x",
                "--run", str(run), "--run", str(run),
                "--confirmations", "2",
                "-o", str(out),
            )
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
