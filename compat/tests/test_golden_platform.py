#!/usr/bin/env python3
"""Unit tests for compat/golden/platforms.py."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "golden"))

from platforms import (  # noqa: E402
    SUPPORTED,
    ConstraintError,
    check_tree,
    file_constraint,
    filename_constraint,
    offending_platforms,
    platforms_for,
    plus_build_to_expr,
)


def constraint(name: str, text: str) -> str | None:
    return file_constraint(Path(name), text)


class ConstraintParsingTests(unittest.TestCase):
    def test_go_build_line_is_read(self):
        self.assertEqual(constraint("x.go", "//go:build linux\n\npackage p\n"), "(linux)")

    def test_go_build_supersedes_the_legacy_form(self):
        """gofmt writes both; only the first decides, as in go/build."""
        text = "//go:build linux || darwin\n// +build linux darwin\n\npackage p\n"
        self.assertEqual(constraint("x.go", text), "(linux || darwin)")

    def test_plus_build_lines_are_anded(self):
        text = "// +build linux\n// +build amd64\n\npackage p\n"
        self.assertEqual(constraint("x.go", text), "((linux)) && ((amd64))")

    def test_plus_build_space_is_or_and_comma_is_and(self):
        self.assertEqual(plus_build_to_expr("linux,amd64 darwin"), "((linux && amd64) || (darwin))")

    def test_plus_build_after_the_package_clause_is_not_a_constraint(self):
        """The shape govet's buildtag/misplaced_plus.go exists to test."""
        text = "package p\n\n// +build linux\n\nfunc f() {}\n"
        self.assertIsNone(constraint("x.go", text))

    def test_no_constraint_at_all(self):
        self.assertIsNone(constraint("x.go", "package p\n"))


class FilenameTests(unittest.TestCase):
    def test_goos_suffix(self):
        self.assertEqual(filename_constraint("bar_linux.go"), "(linux)")

    def test_goos_goarch_suffix(self):
        self.assertEqual(filename_constraint("bar_linux_amd64.go"), "(linux && amd64)")

    def test_test_suffix_is_stripped_first(self):
        self.assertEqual(filename_constraint("bar_linux_test.go"), "(linux)")

    def test_bare_platform_name_is_not_a_suffix(self):
        """Go only reads the suffix when something precedes it."""
        self.assertIsNone(filename_constraint("linux.go"))

    def test_suffix_combines_with_the_header(self):
        text = "//go:build !plan9\n\npackage p\n"
        self.assertEqual(constraint("bar_linux.go", text), "(!plan9) && (linux)")


class InvarianceTests(unittest.TestCase):
    def test_platform_free_constraint_is_invariant(self):
        self.assertIsNone(offending_platforms("!plan9", SUPPORTED))

    def test_goos_constraint_splits_the_matrix(self):
        split = offending_platforms("linux", SUPPORTED)
        self.assertIsNotNone(split)
        self.assertEqual({(o, a) for o, a, v in split if v}, {("linux", "amd64"), ("linux", "arm64")})

    def test_goarch_constraint_splits_the_matrix(self):
        self.assertIsNotNone(offending_platforms("amd64", SUPPORTED))

    def test_unix_is_true_on_every_supported_platform(self):
        """True today only because the matrix has no windows in it."""
        self.assertIsNone(offending_platforms("unix", SUPPORTED))

    def test_opaque_tags_are_tried_both_ways(self):
        self.assertIsNone(offending_platforms("custom", SUPPORTED))
        self.assertIsNone(offending_platforms("go1.24 && !nope", SUPPORTED))

    def test_an_opaque_tag_does_not_excuse_a_platform_tag(self):
        self.assertIsNotNone(offending_platforms("custom && linux", SUPPORTED))

    def test_negations_and_parentheses(self):
        self.assertIsNone(offending_platforms("!(plan9 || windows)", SUPPORTED))
        self.assertIsNotNone(offending_platforms("!(plan9 || darwin)", SUPPORTED))

    def test_unparsable_constraint_is_an_error_not_a_pass(self):
        with self.assertRaises(ConstraintError):
            offending_platforms("linux &&", SUPPORTED)


class PinnedPlatformTests(unittest.TestCase):
    def test_unpinned_is_the_whole_matrix(self):
        self.assertEqual(platforms_for(None, None), SUPPORTED)

    def test_pinning_goos_narrows_the_matrix(self):
        self.assertEqual(platforms_for("linux", None), (("linux", "amd64"), ("linux", "arm64")))

    def test_a_pinned_pair_outside_the_matrix_is_still_one_platform(self):
        """cases/staticcheck-386 cross-compiles to linux/386 on purpose."""
        self.assertEqual(platforms_for("linux", "386"), (("linux", "386"),))
        self.assertIsNone(offending_platforms("linux", platforms_for("linux", "386")))


class TreeTests(unittest.TestCase):
    def test_the_two_fixtures_this_module_was_written_for(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "sa4019").mkdir()
            (root / "sa4019" / "bad.go").write_text(
                "// +build linux\n// +build linux\n\npackage main\n\nfunc main() {}\n"
            )
            (root / "sa4032").mkdir()
            (root / "sa4032" / "bad.go").write_text(
                '//go:build linux\n\npackage main\nimport "runtime"\n'
                'func main() {\n\t_ = runtime.GOOS == "windows"\n}\n'
            )
            problems = check_tree(root, SUPPORTED)
        self.assertEqual(len(problems), 2, problems)

    def test_the_shapes_that_replaced_them_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "sa4019").mkdir()
            (root / "sa4019" / "bad.go").write_text(
                "// +build !plan9\n// +build !plan9\n\npackage main\n\nfunc main() {}\n"
            )
            (root / "sa4032").mkdir()
            (root / "sa4032" / "bad.go").write_text(
                '//go:build !plan9\n\npackage main\nimport "runtime"\n'
                'func main() {\n\t_ = runtime.GOOS == "plan9"\n}\n'
            )
            self.assertEqual(check_tree(root, SUPPORTED), [])


if __name__ == "__main__":
    unittest.main()
