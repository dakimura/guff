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


class ParseTreeDiffTest(unittest.TestCase):
    """The over-write guard reads this, so it has to read a diff, not a prefix."""

    def test_a_removed_line_starting_with_dashes_is_not_a_file_header(self):
        """`--- x` in a hunk is the Go line `-- x`, deleted.

        Splitting on the first three characters would read it as the start of a
        new file called `x`, and every removal after it would be attributed to
        that file — so a guard built on the split would stop seeing the real
        file's over-writes exactly when the fixture contains a comment banner.
        """
        body = (
            "--- a/x.go\n+++ b/x.go\n@@ -1,2 +1,1 @@\n"
            "--- a banner comment\n"
            " package p\n"
        )
        edits = fixdiff.parse_tree_diff(body)
        self.assertEqual(list(edits), ["x.go"])
        self.assertEqual(list(edits["x.go"].removed), ["-- a banner comment"])

    def test_hunk_counts_bound_the_walk(self):
        body = (
            "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n"
            "--- a/y.go\n+++ b/y.go\n@@ -1 +1 @@\n-c\n+d\n"
        )
        edits = fixdiff.parse_tree_diff(body)
        self.assertEqual(sorted(edits), ["x.go", "y.go"])
        self.assertEqual(list(edits["y.go"].removed), ["c"])

    def test_a_missing_newline_marker_is_not_a_diff_line(self):
        body = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n" + fixdiff.NO_NEWLINE + "\n+b\n"
        edits = fixdiff.parse_tree_diff(body)
        self.assertEqual(list(edits["x.go"].removed), ["a"])
        self.assertEqual(list(edits["x.go"].added), ["b"])


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

    def test_divergent_holds_only_with_a_written_reason(self):
        """A deliberate divergence nobody explained is an allowlist entry."""
        body = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n"
        with tempfile.TemporaryDirectory() as tmp:
            actual = Path(tmp) / "actual.diff"
            actual.write_text(body, encoding="utf-8")
            div = Path(tmp) / "x.diff"

            # No `# why:` — refused, even though the bytes match.
            div.write_text(
                "# just because\n# upstream-writes: nothing\n" + body, encoding="utf-8"
            )
            r = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(Path(tmp) / "absent.diff"),
                "--divergent", str(div),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("no `# why:`", r.stderr)

            # With a reason, it holds — and prints the reason on every run.
            div.write_text(
                "# why: upstream is wrong, see notes\n"
                "# upstream-writes: nothing\n" + body,
                encoding="utf-8",
            )
            ok = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(Path(tmp) / "absent.diff"),
                "--divergent", str(div),
            )
            self.assertEqual(ok.returncode, 0, ok.stderr)
            self.assertIn("deliberate divergence", ok.stdout)
            self.assertIn("upstream is wrong", ok.stdout)

    def test_divergent_fails_when_guff_moves(self):
        """It is a record of one decision, not a licence to write anything."""
        with tempfile.TemporaryDirectory() as tmp:
            actual = Path(tmp) / "actual.diff"
            actual.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+DIFFERENT\n", encoding="utf-8"
            )
            div = Path(tmp) / "x.diff"
            div.write_text(
                "# why: upstream is wrong\n"
                "# upstream-writes: nothing\n"
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n",
                encoding="utf-8",
            )
            r = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(Path(tmp) / "absent.diff"),
                "--divergent", str(div),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("DIVERGENCE MOVED", r.stderr)

    def test_divergent_fails_once_upstream_starts_fixing(self):
        """The reason rests on upstream writing nothing. If it does, decide again."""
        body = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n"
        with tempfile.TemporaryDirectory() as tmp:
            actual = Path(tmp) / "actual.diff"
            actual.write_text(body, encoding="utf-8")
            expected = Path(tmp) / "expected.diff"
            expected.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+upstream\n", encoding="utf-8"
            )
            div = Path(tmp) / "x.diff"
            div.write_text(
                "# why: upstream is wrong\n# upstream-writes: nothing\n" + body,
                encoding="utf-8",
            )
            r = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("no longer describes reality", r.stderr)

    def test_pending_refuses_an_over_write_inside_a_case_upstream_does_touch(self):
        """The shape `parens` hid in.

        The older refusal asked one question about the whole case — does
        upstream write anything at all here — and upstream wrote seven hunks in
        `parens`, so guff's eighth was held as a deferral. A gap is guff not
        acting on a finding; this is guff rewriting a line upstream reads and
        leaves, which is the `omitzero` failure with more context around it.
        """
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n", encoding="utf-8"
            )
            actual = Path(tmp) / "actual.diff"
            actual.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
                "@@ -9 +9 @@\n-untouched\n+guff had an opinion\n",
                encoding="utf-8",
            )
            r = self.run_cli(
                "pending", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "-o", str(Path(tmp) / "pending" / "x.diff"),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("REFUSING to hold", r.stderr)
            self.assertIn(
                "guff removes a line golangci-lint --fix keeps: untouched", r.stderr
            )
            self.assertFalse((Path(tmp) / "pending" / "x.diff").exists())

    def test_pending_still_holds_a_different_answer_to_the_same_finding(self):
        """Not every disagreement is an over-write.

        `staticcheck-qf` removes exactly the lines upstream removes and puts
        four different lines back. That is one finding fixed two ways — a real
        gap, and the thing `pending` exists for. A refusal that keyed on added
        lines as well would have thrown it out with the over-writes.
        """
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+upstream\n",
                encoding="utf-8",
            )
            actual = Path(tmp) / "actual.diff"
            actual.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+guff\n", encoding="utf-8"
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

    def test_pending_refuses_a_file_upstream_never_opens(self):
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n", encoding="utf-8"
            )
            actual = Path(tmp) / "actual.diff"
            actual.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
                "--- /dev/null\n+++ b/y.go\n@@ -0,0 +1 @@\n+package p\n",
                encoding="utf-8",
            )
            r = self.run_cli(
                "pending", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "-o", str(Path(tmp) / "pending" / "x.diff"),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("y.go: guff creates a file", r.stderr)

    def test_divergent_holds_a_case_upstream_also_writes_to(self):
        """`parens`: upstream writes one hunk, guff writes that one and a second."""
        upstream = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
        mine = upstream + "@@ -9 +9 @@\n-b\n+B\n"
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(upstream, encoding="utf-8")
            actual = Path(tmp) / "actual.diff"
            actual.write_text(mine, encoding="utf-8")
            div = Path(tmp) / "x.diff"
            declared = fixdiff.upstream_writes(upstream)
            div.write_text(
                f"# why: upstream drops this fix by accident\n"
                f"# upstream-writes: {declared}\n" + mine,
                encoding="utf-8",
            )
            ok = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(ok.returncode, 0, ok.stderr)
            self.assertIn("deliberate divergence", ok.stdout)
            self.assertIn("rewrites 1 thing(s) upstream leaves alone", ok.stdout)

            # Upstream's own output moves — the digest stops matching and the
            # reason has to be read again. This is what `if expected.strip()`
            # used to do, and it only worked while upstream wrote nothing.
            expected.write_text(
                "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+MOVED\n", encoding="utf-8"
            )
            moved = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(moved.returncode, 1)
            self.assertIn("no longer describes reality", moved.stderr)

    def test_divergent_refuses_a_case_that_is_also_a_gap(self):
        """One `# why:` cannot stand in for an edit guff simply does not make."""
        upstream = (
            "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
            "@@ -20 +20 @@\n-missed\n+FIXED\n"
        )
        mine = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n@@ -9 +9 @@\n-b\n+B\n"
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(upstream, encoding="utf-8")
            actual = Path(tmp) / "actual.diff"
            actual.write_text(mine, encoding="utf-8")
            div = Path(tmp) / "x.diff"
            div.write_text(
                f"# why: upstream drops this fix by accident\n"
                f"# upstream-writes: {fixdiff.upstream_writes(upstream)}\n" + mine,
                encoding="utf-8",
            )
            r = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("not only a divergence", r.stderr)
            # Named from upstream's side, not guff's: the same helper answers
            # both directions and the sentence has to say which one it ran.
            self.assertIn(
                "golangci-lint --fix removes a line guff keeps: missed", r.stderr
            )

    def test_divergent_allows_a_subset_when_upstream_breaks_the_build(self):
        """`noinlineerr`: upstream's own output does not parse, so guff writes less.

        Everywhere else this is a gap and belongs in pending/. The difference is
        that a gap is expected to close and this one is not: reproducing it means
        shipping a --fix that breaks its user's build.
        """
        upstream = (
            "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
            "--- a/y.go\n+++ b/y.go\n@@ -1 +1 @@\n-b\n+} else b := f()\n"
        )
        mine = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(upstream, encoding="utf-8")
            actual = Path(tmp) / "actual.diff"
            actual.write_text(mine, encoding="utf-8")
            div = Path(tmp) / "x.diff"
            div.write_text(
                f"# why: upstream inserts at the `if` keyword, which for an"
                f" `else if` sits after `else`\n"
                f"# upstream-writes: {fixdiff.upstream_writes(upstream)}\n"
                f"# upstream-breaks-build: ./y.go:1:9: syntax error\n" + mine,
                encoding="utf-8",
            )
            ok = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(ok.returncode, 0, ok.stderr)
            self.assertIn("does not compile", ok.stdout)

            # The escape hatch is still tied to upstream's output: when that
            # moves, the digest stops matching here exactly as it does for the
            # superset shape, and the reason gets re-read.
            expected.write_text(upstream.replace("+A", "+MOVED"), encoding="utf-8")
            moved = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(moved.returncode, 1)
            self.assertIn("no longer describes reality", moved.stderr)

    def test_breaking_the_build_does_not_license_an_over_write(self):
        """Writing *less* is what the claim buys. Writing *more* is not."""
        upstream = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
        mine = upstream + "--- a/z.go\n+++ b/z.go\n@@ -1 +1 @@\n-untouched\n+MINE\n"
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(upstream, encoding="utf-8")
            actual = Path(tmp) / "actual.diff"
            actual.write_text(mine, encoding="utf-8")
            div = Path(tmp) / "x.diff"
            div.write_text(
                f"# why: upstream's fixer emits code that does not parse\n"
                f"# upstream-writes: {fixdiff.upstream_writes(upstream)}\n"
                f"# upstream-breaks-build: ./y.go:1:9: syntax error\n" + mine,
                encoding="utf-8",
            )
            r = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("which it does not buy", r.stderr)

    def test_breaks_build_claim_has_to_say_what_breaks(self):
        """An empty claim is the deferral note that outlives its own reason."""
        upstream = (
            "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
            "--- a/y.go\n+++ b/y.go\n@@ -1 +1 @@\n-b\n+} else b := f()\n"
        )
        mine = "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+A\n"
        with tempfile.TemporaryDirectory() as tmp:
            expected = Path(tmp) / "expected.diff"
            expected.write_text(upstream, encoding="utf-8")
            actual = Path(tmp) / "actual.diff"
            actual.write_text(mine, encoding="utf-8")
            div = Path(tmp) / "x.diff"
            div.write_text(
                f"# why: upstream's fixer emits code that does not parse\n"
                f"# upstream-writes: {fixdiff.upstream_writes(upstream)}\n"
                f"# upstream-breaks-build:\n" + mine,
                encoding="utf-8",
            )
            r = self.run_cli(
                "check", "--case", "x",
                "--actual", str(actual),
                "--expected", str(expected),
                "--divergent", str(div),
            )
            self.assertEqual(r.returncode, 1)
            self.assertIn("Quote the compiler error", r.stderr)

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
