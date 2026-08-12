package main

// Mutation sites for the differential fuzzer (COMPAT-HARDENING Phase 6).
//
// `spans` answers "what can be deleted". This answers "what can be changed
// without deleting anything", which is the other half of Phase 6: hand-written
// fixtures only cover shapes somebody thought of, and the shapes that produced
// this repo's bugs were mostly ones nobody would think to write.
//
// A mutation here has exactly one obligation: **the result must still compile**.
// It does not have to preserve findings, and deliberately does not try to. The
// fuzzer compares guff against golangci-lint on the *mutant*, so a mutation that
// changes what both tools report is just as useful as one that changes nothing —
// what is being tested is whether the two still agree. That is what makes this
// affordable: no mutation needs a semantic-equivalence argument, only a
// syntactic one, and the Go toolchain checks it for free.
//
// Every site is a byte-range replacement, never a reprint of the file. Printing
// the AST back out would reformat the whole file: `gofmt`'s findings would move,
// and every column in every other finding would shift, so a single mutation
// would light up the entire diff and tell us nothing about which change caused
// what.
//
// Sites are emitted for the shapes with the worst track record in this codebase:
//
//	paren    — `x` -> `(x)`. Upstream checkers call `ast.Unparen` in some paths
//	           and not others; guff's ports inherit whichever the author read.
//	comment  — a comment line before a statement. "The analysis AST has no
//	           comments" was diagnosed eight separate times in §4 (buildtag,
//	           directive, comments-density, comment-spacings, ...), each time as
//	           a fresh bug, so this shape is worth generating rather than waiting
//	           for a corpus repo to supply it.
//	nolint   — `//nolint` appended to a line. The five suppression rules are
//	           Phase 4 work; this points them at every line of every fixture.
//	swap     — two adjacent statements exchanged, which moves findings between
//	           lines and catches position and ordering assumptions.
//	varform  — `x := v` <-> `var x = v`. Same program, different AST node, and
//	           checkers routinely match on one node kind only.

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"strings"
)

func mutationsFor(path string) FileSpans {
	res := FileSpans{Path: path}
	src, err := os.ReadFile(path)
	if err != nil {
		res.Error = err.Error()
		return res
	}
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, path, src, parser.ParseComments|parser.SkipObjectResolution)
	if err != nil {
		res.Error = err.Error()
		return res
	}
	tf := fset.File(f.Pos())
	if tf == nil {
		res.Error = "no token.File"
		return res
	}
	m := &mutator{collector: collector{src: src, tf: tf}, commented: commentedLines(fset, f)}
	m.walk(f)
	res.Spans = m.spans
	return res
}

type mutator struct {
	collector
	// Lines that already carry a comment. Appending `//nolint` to one would
	// either land inside a `/* */` or produce two comments on a line, and the
	// second is a different shape from the one being tested.
	commented map[int]bool
}

func commentedLines(fset *token.FileSet, f *ast.File) map[int]bool {
	out := map[int]bool{}
	for _, cg := range f.Comments {
		start := fset.Position(cg.Pos()).Line
		end := fset.Position(cg.End()).Line
		for l := start; l <= end; l++ {
			out[l] = true
		}
	}
	return out
}

func (m *mutator) emit(kind string, start, end int, replace, label string) {
	if start < 0 || end < start || end > len(m.src) {
		return
	}
	m.spans = append(m.spans, Span{
		Kind: kind, Start: start, End: end, Replace: replace, Label: label,
	})
}

// paren wraps an expression in parentheses in place.
func (m *mutator) paren(e ast.Expr) {
	if e == nil {
		return
	}
	switch e.(type) {
	case *ast.ParenExpr:
		return // already parenthesized; doubling it adds nothing
	case *ast.CompositeLit, *ast.FuncLit, *ast.Ellipsis, *ast.KeyValueExpr, *ast.BadExpr:
		// `(T){...}` is not a composite literal and `(func(){})()` changes what
		// the call applies to. Neither is worth the special-casing.
		return
	}
	s, en := m.off(e.Pos()), m.off(e.End())
	if s < 0 || en <= s {
		return
	}
	m.emit("paren", s, en, "("+string(m.src[s:en])+")", exprHead(e))
}

func (m *mutator) walk(f *ast.File) {
	ast.Inspect(f, func(n ast.Node) bool {
		switch n := n.(type) {
		case *ast.BinaryExpr:
			m.paren(n.X)
			m.paren(n.Y)
		case *ast.CallExpr:
			// Not n.Fun: parenthesizing the callee of a conversion or of a
			// generic instantiation changes how it parses.
			for _, a := range n.Args {
				m.paren(a)
			}
		case *ast.ReturnStmt:
			for _, r := range n.Results {
				m.paren(r)
			}
		case *ast.IfStmt:
			m.paren(n.Cond)
		case *ast.ForStmt:
			m.paren(n.Cond)
		case *ast.AssignStmt:
			for _, r := range n.Rhs {
				m.paren(r)
			}
			m.varform(n)
		case *ast.BlockStmt:
			m.stmtSites(n.List)
		case *ast.CaseClause:
			m.stmtSites(n.Body)
		case *ast.CommClause:
			m.stmtSites(n.Body)
		}
		return true
	})
	m.genDeclSites(f)
}

// varform turns `x := v` into `var x = v` and back.
//
// The two are the same program and a different AST node, which is exactly the
// distinction a checker written against one of them gets wrong.
func (m *mutator) varform(a *ast.AssignStmt) {
	if a.Tok != token.DEFINE || len(a.Lhs) != len(a.Rhs) || len(a.Lhs) == 0 {
		return
	}
	for _, l := range a.Lhs {
		if id, ok := l.(*ast.Ident); !ok || id.Name == "_" {
			return // `var _ = v` is legal but a different shape again
		}
	}
	s, e := m.off(a.Pos()), m.off(a.End())
	if s < 0 || e <= s {
		return
	}
	text := string(m.src[s:e])
	// Only the simple one-line form; rewriting a multi-line RHS by hand risks
	// producing something that parses differently than it reads.
	if strings.Contains(text, "\n") || !strings.Contains(text, ":=") {
		return
	}
	m.emit("varform", s, e, "var "+strings.Replace(text, ":=", "=", 1), "define")
}

// genDeclSites turns `var x = v` back into `x := v`, inside functions only.
func (m *mutator) genDeclSites(f *ast.File) {
	ast.Inspect(f, func(n ast.Node) bool {
		fn, ok := n.(*ast.FuncDecl)
		if !ok || fn.Body == nil {
			return true
		}
		ast.Inspect(fn.Body, func(n ast.Node) bool {
			ds, ok := n.(*ast.DeclStmt)
			if !ok {
				return true
			}
			gd, ok := ds.Decl.(*ast.GenDecl)
			if !ok || gd.Tok != token.VAR || gd.Lparen.IsValid() || len(gd.Specs) != 1 {
				return true
			}
			vs, ok := gd.Specs[0].(*ast.ValueSpec)
			if !ok || vs.Type != nil || len(vs.Values) == 0 || len(vs.Names) != len(vs.Values) {
				return true
			}
			s, e := m.off(ds.Pos()), m.off(ds.End())
			if s < 0 || e <= s {
				return true
			}
			text := string(m.src[s:e])
			if strings.Contains(text, "\n") || !strings.HasPrefix(text, "var ") {
				return true
			}
			m.emit("varform", s, e, strings.Replace(text[len("var "):], "=", ":=", 1), "var")
			return true
		})
		return false
	})
}

// stmtSites emits the per-statement mutations: a comment line before it, a
// `//nolint` after it, and a swap with the statement that follows.
func (m *mutator) stmtSites(list []ast.Stmt) {
	for i, s := range list {
		start, end := m.off(s.Pos()), m.off(s.End())
		if start < 0 || end <= start {
			return
		}
		lineStart := start
		for lineStart > 0 && m.src[lineStart-1] != '\n' {
			lineStart--
		}
		indent := string(m.src[lineStart:start])
		if strings.TrimSpace(indent) != "" {
			continue // not the first thing on its line
		}
		line := m.tf.Line(s.Pos())

		m.emit("comment", lineStart, lineStart,
			indent+"// mutation: a comment where the AST has none\n", fmt.Sprintf("L%d", line))

		// `//nolint` with no linter list suppresses every linter on the line,
		// which is the broadest of the five rules and the one worth firing at
		// every statement.
		endLine := m.tf.Line(s.End())
		if !m.commented[line] && !m.commented[endLine] {
			eol := end
			for eol < len(m.src) && m.src[eol] != '\n' {
				eol++
			}
			if strings.TrimSpace(string(m.src[end:eol])) == "" {
				m.emit("nolint", end, eol, " //nolint", fmt.Sprintf("L%d", line))
			}
		}

		if i+1 < len(list) {
			m.swap(s, list[i+1])
		}
	}
}

// swap exchanges two adjacent statements that each occupy whole lines.
func (m *mutator) swap(a, b ast.Stmt) {
	as, ae := m.off(a.Pos()), m.off(a.End())
	bs, be := m.off(b.Pos()), m.off(b.End())
	if as < 0 || bs < 0 || ae <= as || be <= bs || bs < ae {
		return
	}
	as, ae = m.widen(as, ae)
	bs, be = m.widen(bs, be)
	if ae > bs {
		return // widening overlapped them; they share a line
	}
	gap := string(m.src[ae:bs])
	if strings.TrimSpace(gap) != "" {
		return // a comment or a directive sits between them
	}
	first, second := string(m.src[as:ae]), string(m.src[bs:be])
	if !strings.HasSuffix(first, "\n") || !strings.HasSuffix(second, "\n") {
		return
	}
	m.emit("swap", as, be, second+gap+first, "")
}

func exprHead(e ast.Expr) string {
	switch e := e.(type) {
	case *ast.Ident:
		return e.Name
	case *ast.BasicLit:
		return e.Kind.String()
	case *ast.SelectorExpr:
		return e.Sel.Name
	case *ast.CallExpr:
		return "call"
	}
	return fmt.Sprintf("%T", e)
}
