"""Offline unit tests for regress/fmt_diff.py (no native binary required)."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

REGRESS = Path(__file__).resolve().parents[1]
MOD_PATH = REGRESS / "fmt_diff.py"


def _load():
    spec = importlib.util.spec_from_file_location("fmt_diff", MOD_PATH)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    sys.modules["fmt_diff"] = mod
    spec.loader.exec_module(mod)
    return mod


class CollectGoFiles(unittest.TestCase):
    def test_skips_vendor_and_testdata_dirs(self):
        fmt_diff = _load()
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "ok.go").write_text("package p\n", encoding="utf-8")
            (root / "vendor").mkdir()
            (root / "vendor" / "x.go").write_text("package v\n", encoding="utf-8")
            (root / "testdata").mkdir()
            (root / "testdata" / "bad.go").write_text("not go\n", encoding="utf-8")
            (root / "sub").mkdir()
            (root / "sub" / "a.go").write_text("package s\n", encoding="utf-8")
            files = fmt_diff.collect_go_files([root], limit=None)
            names = sorted(p.name for p in files)
            self.assertEqual(names, ["a.go", "ok.go"])

    def test_limit_truncates_sorted(self):
        fmt_diff = _load()
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            for name in ("c.go", "a.go", "b.go"):
                (root / name).write_text("package p\n", encoding="utf-8")
            files = fmt_diff.collect_go_files([root], limit=2)
            self.assertEqual([p.name for p in files], ["a.go", "b.go"])


class BuildCmds(unittest.TestCase):
    def test_gofmt_simplify(self):
        fmt_diff = _load()
        ref = fmt_diff.RefArgs(formatter="gofmt", simplify=True)
        self.assertEqual(fmt_diff.build_ref_cmd(ref, Path("x.go"), None), ["gofmt", "-s"])

    def test_gofumpt_extra(self):
        fmt_diff = _load()
        ref = fmt_diff.RefArgs(formatter="gofumpt", extra=True, lang="go1.22")
        self.assertEqual(
            fmt_diff.build_ref_cmd(ref, Path("x.go"), None),
            ["gofumpt", "-extra", "-lang", "go1.22"],
        )

    def test_native_mirrors_flags(self):
        fmt_diff = _load()
        ref = fmt_diff.RefArgs(
            formatter="goimports",
            local="github.com/prometheus/prometheus",
        )
        cmd = fmt_diff.build_native_cmd(
            Path("/tmp/guff-fmt-native"), ref, Path("/abs/x.go")
        )
        self.assertEqual(
            cmd,
            [
                "/tmp/guff-fmt-native",
                "goimports",
                "--local",
                "github.com/prometheus/prometheus",
                "--filename",
                "/abs/x.go",
            ],
        )


if __name__ == "__main__":
    unittest.main()
