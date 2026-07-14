#!/usr/bin/env bash
# Regenerate benchmarks/local (committed synthetic corpus).
# Kept simple enough for guff staticcheck/SSA today: no switch / ++ / --.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 - "$ROOT/benchmarks/local" <<'PY'
from pathlib import Path
import shutil, sys
root = Path(sys.argv[1])
if root.exists():
    shutil.rmtree(root)
root.mkdir(parents=True)
(root / "go.mod").write_text("module github.com/dakimura/guff/benchmarks/local\n\ngo 1.22\n")
pkgs, files_per = 12, 8
for i in range(pkgs):
    pkg = root / f"pkg{i:02d}"
    pkg.mkdir()
    for j in range(files_per):
        if j == 0:
            body = f'''package pkg{i:02d}

import "fmt"

func mayErr{j}() error {{
	return fmt.Errorf("e")
}}

func Use{j}() {{
	_ = fmt.Sprintf("%d", {i*100+j})
}}

func CallUnchecked{j}() {{
	mayErr{j}() // want errcheck
}}
'''
        elif j == 1:
            body = f'''package pkg{i:02d}

func Ineff{j}() int {{
	x := 1
	x = 2 // want ineffassign
	return x
}}

func helperUnused{j}() int {{ // want unused
	return {i+j}
}}
'''
        else:
            body = f'''package pkg{i:02d}

func Work{j}(n int) int {{
	sum := 0
	k := 0
	for k < n {{
		if k%3 == 0 {{
			sum = sum + k
		}} else if k%3 == 1 {{
			sum = sum + k*2
		}} else {{
			sum = sum - 1
		}}
		k = k + 1
	}}
	if sum < 0 {{
		return -sum
	}}
	return sum + {i*10+j}
}}

func Build{j}(xs []int) []int {{
	out := make([]int, 0, len(xs))
	for _, v := range xs {{
		if v%2 == 0 {{
			out = append(out, v)
		}}
	}}
	return out
}}

func Map{j}(xs []string) int {{
	n := 0
	for _, s := range xs {{
		if len(s) > 0 {{
			n = n + len(s)
		}}
	}}
	return n
}}
'''
        (pkg / f"f{j:02d}.go").write_text(body)
main = "package main\n\nimport (\n"
for i in range(pkgs):
    main += f'\t"github.com/dakimura/guff/benchmarks/local/pkg{i:02d}"\n'
main += ")\n\nfunc main() {\n"
for i in range(pkgs):
    main += f"\t_ = pkg{i:02d}.Work2(10)\n"
    main += f"\tpkg{i:02d}.Use0()\n"
main += "}\n"
(root / "main.go").write_text(main)
n = sum(1 for _ in root.rglob("*.go"))
loc = sum(len(p.read_text().splitlines()) for p in root.rglob("*.go"))
print(f"wrote {n} files / {loc} LOC -> {root}")
PY
