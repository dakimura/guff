//go:build ignore

// gen-ginkgo-stub writes a declarations-only copy of the Ginkgo / Gomega
// packages that ginkgolinter's upstream testdata imports.
//
//	go run scripts/gen-ginkgo-stub.go <vendor-dir> <out-dir> <pkg>...
//
// ginkgolinter v0.24's testdata/src/a vendors ginkgo, gomega and their
// dependencies — 16 MB, too large to check in. The linter only reads types and
// signatures, so every function body is replaced with `panic("stub")`, comments
// (and with them build constraints and go:embed directives) are dropped, and
// imports left unused are removed. Whatever a remaining declaration still
// imports is stubbed too, transitively. Files are selected for linux/amd64
// without cgo, so the stub carries no build tags.
//
// Each module under <vendor-dir> (per vendor/modules.txt) becomes a directory
// <out-dir>/<module path> with its own go.mod, for a `replace` in the case.
package main

import (
	"bufio"
	"bytes"
	"fmt"
	"go/ast"
	"go/build"
	"go/parser"
	"go/printer"
	"go/token"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

func main() {
	if len(os.Args) < 4 {
		fmt.Fprintln(os.Stderr, "usage: gen-ginkgo-stub <vendor-dir> <out-dir> <pkg>...")
		os.Exit(2)
	}
	vendor, out := os.Args[1], os.Args[2]
	modules := readModules(filepath.Join(vendor, "modules.txt"))

	ctx := build.Default
	ctx.GOOS, ctx.GOARCH, ctx.CgoEnabled = "linux", "amd64", false

	seen := map[string]bool{}
	queue := append([]string(nil), os.Args[3:]...)
	usedMods := map[string]bool{}
	for len(queue) > 0 {
		pkg := queue[0]
		queue = queue[1:]
		if seen[pkg] {
			continue
		}
		seen[pkg] = true
		mod := moduleOf(modules, pkg)
		if mod == "" {
			fatal("no vendored module provides %s", pkg)
		}
		usedMods[mod] = true
		dir := filepath.Join(vendor, filepath.FromSlash(pkg))
		bp, err := ctx.ImportDir(dir, 0)
		if err != nil {
			fatal("%s: %v", pkg, err)
		}
		for _, name := range bp.GoFiles {
			imports := stubFile(filepath.Join(dir, name), filepath.Join(out, filepath.FromSlash(pkg), name))
			for _, imp := range imports {
				if !isStd(imp) && !seen[imp] {
					queue = append(queue, imp)
				}
			}
		}
	}
	var mods []string
	for m := range usedMods {
		mods = append(mods, m)
	}
	sort.Strings(mods)
	for _, m := range mods {
		gomod := fmt.Sprintf("module %s\n\ngo 1.22.0\n", m)
		write(filepath.Join(out, filepath.FromSlash(m), "go.mod"), []byte(gomod))
		fmt.Println(m)
	}
}

// stubFile writes src without function bodies or comments and returns the
// import paths the result still uses.
func stubFile(src, dst string) []string {
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, src, nil, parser.SkipObjectResolution)
	if err != nil {
		fatal("%v", err)
	}
	for _, d := range f.Decls {
		if fd, ok := d.(*ast.FuncDecl); ok && fd.Body != nil {
			fd.Body = &ast.BlockStmt{List: []ast.Stmt{&ast.ExprStmt{X: &ast.CallExpr{
				Fun:  ast.NewIdent("panic"),
				Args: []ast.Expr{&ast.BasicLit{Kind: token.STRING, Value: `"stub"`}},
			}}}}
		}
	}
	f.Comments = nil
	f.Doc = nil

	used := map[string]bool{}
	ast.Inspect(f, func(n ast.Node) bool {
		if sel, ok := n.(*ast.SelectorExpr); ok {
			if id, ok := sel.X.(*ast.Ident); ok {
				used[id.Name] = true
			}
		}
		return true
	})
	var keep []string
	var specs []ast.Spec
	for _, imp := range f.Imports {
		p, _ := strconv.Unquote(imp.Path.Value)
		name := path.Base(p)
		if imp.Name != nil {
			name = imp.Name.Name
		} else if strings.HasPrefix(name, "v") && strings.Contains(p, "/") {
			if _, err := strconv.Atoi(name[1:]); err == nil {
				name = path.Base(path.Dir(p))
			}
		}
		// Blank imports only matter to bodies. A dot import cannot be traced
		// by name; none of the stubbed declarations needs one.
		if name == "_" || name == "." {
			continue
		}
		if used[name] || used[strings.ReplaceAll(name, "-", "_")] {
			keep = append(keep, p)
			imp.Doc, imp.Comment = nil, nil
			specs = append(specs, imp)
		}
	}
	var decls []ast.Decl
	for _, d := range f.Decls {
		if gd, ok := d.(*ast.GenDecl); ok && gd.Tok == token.IMPORT {
			continue
		}
		decls = append(decls, d)
	}
	if len(specs) > 0 {
		decls = append([]ast.Decl{&ast.GenDecl{Tok: token.IMPORT, Lparen: 1, Specs: specs}}, decls...)
	}
	f.Decls = decls

	var buf bytes.Buffer
	if err := (&printer.Config{Mode: printer.UseSpaces | printer.TabIndent, Tabwidth: 8}).Fprint(&buf, token.NewFileSet(), f); err != nil {
		fatal("%s: %v", src, err)
	}
	write(dst, buf.Bytes())
	return keep
}

func readModules(p string) []string {
	fh, err := os.Open(p)
	if err != nil {
		fatal("%v", err)
	}
	defer fh.Close()
	var mods []string
	sc := bufio.NewScanner(fh)
	for sc.Scan() {
		line := sc.Text()
		if strings.HasPrefix(line, "# ") {
			mods = append(mods, strings.Fields(line)[1])
		}
	}
	return mods
}

func moduleOf(mods []string, pkg string) string {
	best := ""
	for _, m := range mods {
		if (pkg == m || strings.HasPrefix(pkg, m+"/")) && len(m) > len(best) {
			best = m
		}
	}
	return best
}

func isStd(p string) bool {
	first, _, _ := strings.Cut(p, "/")
	return !strings.Contains(first, ".")
}

func write(p string, b []byte) {
	if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
		fatal("%v", err)
	}
	if err := os.WriteFile(p, b, 0o644); err != nil {
		fatal("%v", err)
	}
}

func fatal(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "gen-ginkgo-stub: "+format+"\n", args...)
	os.Exit(1)
}
