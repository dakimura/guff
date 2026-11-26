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
	m := &mutator{
		collector:  collector{src: src, tf: tf},
		commented:  commentedLines(fset, f),
		headerStmt: headerStmts(f),
	}
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
	// Assignments in a header slot — `for x := 0; ...`, `if x := f(); ...`.
	// Go's grammar allows a SimpleStmt there and nothing else, so rewriting one
	// into a `var` declaration cannot compile. The build would reject it, but a
	// mutant rejected by construction is a wasted round trip through two
	// linters, and 4.5% of the first full run went that way.
	headerStmt map[ast.Stmt]bool
}

func headerStmts(f *ast.File) map[ast.Stmt]bool {
	out := map[ast.Stmt]bool{}
	mark := func(s ast.Stmt) {
		if s != nil {
			out[s] = true
		}
	}
	ast.Inspect(f, func(n ast.Node) bool {
		switch n := n.(type) {
		case *ast.ForStmt:
			mark(n.Init)
			mark(n.Post)
		case *ast.IfStmt:
			mark(n.Init)
		case *ast.SwitchStmt:
			mark(n.Init)
		case *ast.TypeSwitchStmt:
			mark(n.Init)
			mark(n.Assign)
		}
		return true
	})
	return out
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
	m.renameSites(f)
	m.litTypeSites(f)
	m.rangeIntSites(f)
}

// varform turns `x := v` into `var x = v` and back.
//
// The two are the same program and a different AST node, which is exactly the
// distinction a checker written against one of them gets wrong.
func (m *mutator) varform(a *ast.AssignStmt) {
	if a.Tok != token.DEFINE || len(a.Lhs) != len(a.Rhs) || len(a.Lhs) == 0 {
		return
	}
	if m.headerStmt[a] {
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

// --- type-shaped mutations, without a type checker ---------------------------
//
// The three shapes below were parked in COMPAT-HARDENING Phase 6 as "needs type
// information, so decide first whether gospans grows go/types or the work moves
// to Rust". Neither is necessary, and the reason is the invariant at the top of
// this file: **a mutation only has to compile.** It does not have to be
// meaning-preserving, so it does not have to be *correct* either — an edit that
// would need types to justify can simply be attempted, and `go build` throws it
// out when the guess was wrong. Loading types here would buy soundness for
// edits whose unsoundness is already free to detect, at the cost of an importer
// on every pass of a fuzzer that runs thousands of them.
//
// So each of these takes the syntactic subset where the answer is written in
// the source, and lets the toolchain reject the rest:
//
//	rename    — a local whose every occurrence in the file lies inside one
//	            function. Renaming to `len`, `fmt` or an exported spelling is
//	            what predeclared / builtinShadow / importShadow / var-naming /
//	            unexported-naming actually key on; `importShadow`'s scan range
//	            was a Phase 5 bug and nothing generates the shape.
//	littype   — `x := 1` <-> `var x int = 1`. The type of a basic literal is its
//	            token kind, and the type of `T{...}` is spelled in the literal.
//	            ST1023, revive var-declaration and S1021 all live on this line.
//	rangeint  — `for i := 0; i < 10; i++` -> `for i := range 10`. Only over an
//	            integer literal, and only compiles on go1.22+; on an older module
//	            the build rejects it, which is the correct answer for free.

// identOccurrences returns every whole-identifier occurrence of name in the file.
func (m *mutator) identOccurrences(f *ast.File, name string) []*ast.Ident {
	var out []*ast.Ident
	ast.Inspect(f, func(n ast.Node) bool {
		if id, ok := n.(*ast.Ident); ok && id.Name == name {
			out = append(out, id)
		}
		return true
	})
	return out
}

// renameTargets are the spellings worth renaming *to*. Each is a name some
// check keys on: a predeclared identifier, a common import name, and a case
// flip for the exported/unexported rules.
var renameTargets = []string{"len", "fmt", "err"}

func (m *mutator) renameSites(f *ast.File) {
	for _, decl := range f.Decls {
		fn, ok := decl.(*ast.FuncDecl)
		if !ok || fn.Body == nil {
			continue
		}
		fs, fe := m.off(fn.Pos()), m.off(fn.End())
		if fs < 0 || fe <= fs {
			continue
		}
		seen := map[string]bool{}
		ast.Inspect(fn.Body, func(n ast.Node) bool {
			var names []*ast.Ident
			switch n := n.(type) {
			case *ast.AssignStmt:
				if n.Tok != token.DEFINE {
					return true
				}
				for _, l := range n.Lhs {
					if id, ok := l.(*ast.Ident); ok {
						names = append(names, id)
					}
				}
			case *ast.ValueSpec:
				names = n.Names
			}
			for _, id := range names {
				if id.Name == "_" || seen[id.Name] {
					continue
				}
				seen[id.Name] = true
				occ := m.identOccurrences(f, id.Name)
				// Every mention has to be inside this function, or a rename here
				// leaves a dangling reference elsewhere in the file. (A mention
				// in *another* file of the package is not visible from here; that
				// one the build catches.)
				contained := true
				for _, o := range occ {
					if off := m.off(o.Pos()); off < fs || off >= fe {
						contained = false
						break
					}
				}
				if !contained {
					continue
				}
				for _, to := range append(append([]string{}, renameTargets...), flipCase(id.Name)) {
					if to == "" || to == id.Name {
						continue
					}
					m.emit("rename", fs, fe, m.substitute(occ, fs, fe, to),
						id.Name+"->"+to)
				}
			}
			return true
		})
	}
}

// substitute rewrites src[start:end) with each occurrence replaced by to.
func (m *mutator) substitute(occ []*ast.Ident, start, end int, to string) string {
	var b strings.Builder
	prev := start
	for _, o := range occ {
		s, e := m.off(o.Pos()), m.off(o.End())
		if s < prev || e > end {
			continue
		}
		b.Write(m.src[prev:s])
		b.WriteString(to)
		prev = e
	}
	b.Write(m.src[prev:end])
	return b.String()
}

func flipCase(name string) string {
	if name == "" {
		return ""
	}
	first := name[:1]
	up, low := strings.ToUpper(first), strings.ToLower(first)
	if first == up && first != low {
		return low + name[1:]
	}
	if first == low && first != up {
		return up + name[1:]
	}
	return ""
}

// litType names the type of an expression whose type is written in the source.
func litType(e ast.Expr) string {
	switch e := e.(type) {
	case *ast.BasicLit:
		switch e.Kind {
		case token.INT:
			return "int"
		case token.FLOAT:
			return "float64"
		case token.IMAG:
			return "complex128"
		case token.CHAR:
			return "rune"
		case token.STRING:
			return "string"
		}
	case *ast.CompositeLit:
		if e.Type == nil {
			return ""
		}
		switch e.Type.(type) {
		case *ast.Ident, *ast.ArrayType, *ast.MapType, *ast.SelectorExpr:
			return "" // spelled out below from the source bytes
		}
	}
	return ""
}

// litTypeSites turns `x := <lit>` into `var x T = <lit>` and `var x T = v` into
// `var x = v`. Both directions matter: one check reports the redundant type and
// another reports its absence.
func (m *mutator) litTypeSites(f *ast.File) {
	ast.Inspect(f, func(n ast.Node) bool {
		switch n := n.(type) {
		case *ast.AssignStmt:
			if n.Tok != token.DEFINE || len(n.Lhs) != 1 || len(n.Rhs) != 1 || m.headerStmt[n] {
				return true
			}
			id, ok := n.Lhs[0].(*ast.Ident)
			if !ok || id.Name == "_" {
				return true
			}
			typ := litType(n.Rhs[0])
			if typ == "" {
				if cl, ok := n.Rhs[0].(*ast.CompositeLit); ok && cl.Type != nil {
					ts, te := m.off(cl.Type.Pos()), m.off(cl.Type.End())
					if ts >= 0 && te > ts {
						typ = string(m.src[ts:te])
					}
				}
			}
			if typ == "" || strings.Contains(typ, "\n") {
				return true
			}
			s, e := m.off(n.Pos()), m.off(n.End())
			if s < 0 || e <= s {
				return true
			}
			text := string(m.src[s:e])
			if strings.Contains(text, "\n") || !strings.Contains(text, ":=") {
				return true
			}
			m.emit("littype", s, e,
				"var "+strings.Replace(text, ":=", typ+" =", 1), id.Name+":"+typ)
		case *ast.ValueSpec:
			// `var x T = v` -> `var x = v`: exactly what ST1023 and revive's
			// var-declaration report, and neither fires without the type there.
			if n.Type == nil || len(n.Values) == 0 {
				return true
			}
			ts, te := m.off(n.Type.Pos()), m.off(n.Type.End())
			if ts < 0 || te <= ts || te >= len(m.src) {
				return true
			}
			// Swallow the single space that follows the type, if any.
			cut := te
			if m.src[cut] == ' ' {
				cut++
			}
			m.emit("littype", ts, cut, "", "drop-type")
		}
		return true
	})
}

// rangeIntSites rewrites the counted loop into `range N`, which only parses as
// an integer range on go1.22+. On an older module the build rejects it.
func (m *mutator) rangeIntSites(f *ast.File) {
	ast.Inspect(f, func(n ast.Node) bool {
		fs, ok := n.(*ast.ForStmt)
		if !ok || fs.Init == nil || fs.Cond == nil || fs.Post == nil {
			return true
		}
		init, ok := fs.Init.(*ast.AssignStmt)
		if !ok || init.Tok != token.DEFINE || len(init.Lhs) != 1 || len(init.Rhs) != 1 {
			return true
		}
		name, ok := init.Lhs[0].(*ast.Ident)
		if !ok {
			return true
		}
		if lit, ok := init.Rhs[0].(*ast.BasicLit); !ok || lit.Kind != token.INT || lit.Value != "0" {
			return true
		}
		cond, ok := fs.Cond.(*ast.BinaryExpr)
		if !ok || cond.Op != token.LSS {
			return true
		}
		if x, ok := cond.X.(*ast.Ident); !ok || x.Name != name.Name {
			return true
		}
		limit, ok := cond.Y.(*ast.BasicLit)
		if !ok || limit.Kind != token.INT {
			return true
		}
		inc, ok := fs.Post.(*ast.IncDecStmt)
		if !ok || inc.Tok != token.INC {
			return true
		}
		if x, ok := inc.X.(*ast.Ident); !ok || x.Name != name.Name {
			return true
		}
		s, e := m.off(fs.Init.Pos()), m.off(fs.Post.End())
		if s < 0 || e <= s {
			return true
		}
		m.emit("rangeint", s, e, name.Name+" := range "+limit.Value, name.Name)
		return true
	})
}
