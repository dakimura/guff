// Command gospans reports the byte spans a Go source file can be shrunk by.
//
// It is the syntax-aware half of `compat/reduce.py` (COMPAT-HARDENING Phase 6).
// The reducer itself is language-agnostic delta debugging over a set of spans;
// this program is what makes those spans *Go declarations* rather than lines.
// The difference matters in both directions:
//
//   - A line-based reducer cannot delete one method of an interface, one field
//     of a struct, or one arm of a `var (...)` block without leaving the file
//     unparseable, so it stalls at the first brace it cannot balance.
//   - It also cannot make the one non-deleting move that shrinks a repro the
//     most: replacing a function *body* with `panic(...)`, which keeps the
//     signature (and therefore every caller and every interface it satisfies)
//     while dropping everything inside.
//
// Output is a JSON array, one object per file, with spans in **byte offsets**
// into that file. Offsets, not line numbers, because the reducer edits bytes
// and re-parses; a line number stops being true after the first edit.
//
// Usage:
//
//	gospans spans FILE...        # removable spans, as JSON
//	gospans parses FILE          # exit 0 iff FILE still parses
//
// Only the standard library is used, so this builds with no module downloads.
package main

import (
	"encoding/json"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"strconv"
	"strings"
)

// Span is one candidate edit: replace src[Start:End] with Replace.
//
// Deletion is Replace == "". The only non-empty Replace this program emits is
// the `panic(...)` body above, but the reducer treats the field generically so
// a future mutation pass can reuse the same plumbing.
type Span struct {
	Kind    string `json:"kind"`
	Start   int    `json:"start"`
	End     int    `json:"end"`
	Replace string `json:"replace,omitempty"`
	Label   string `json:"label,omitempty"`
}

// FileSpans is the per-file result. Path is echoed back so the reducer can
// pass many files in one process launch — it reduces whole modules, and one
// exec per file per pass dominated the runtime when that was the shape.
type FileSpans struct {
	Path  string `json:"path"`
	Spans []Span `json:"spans"`
	Error string `json:"error,omitempty"`
}

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr,
			"usage: gospans spans FILE... | gospans mutations FILE... | gospans parses FILE")
		os.Exit(2)
	}
	switch os.Args[1] {
	case "spans", "mutations":
		out := make([]FileSpans, 0, len(os.Args)-2)
		for _, path := range os.Args[2:] {
			if os.Args[1] == "spans" {
				out = append(out, spansFor(path))
			} else {
				out = append(out, mutationsFor(path))
			}
		}
		enc := json.NewEncoder(os.Stdout)
		enc.SetIndent("", "  ")
		if err := enc.Encode(out); err != nil {
			fmt.Fprintln(os.Stderr, "gospans:", err)
			os.Exit(1)
		}
	case "parses":
		if len(os.Args) != 3 {
			fmt.Fprintln(os.Stderr, "usage: gospans parses FILE")
			os.Exit(2)
		}
		fset := token.NewFileSet()
		if _, err := parser.ParseFile(fset, os.Args[2], nil, parser.SkipObjectResolution); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	default:
		fmt.Fprintf(os.Stderr, "gospans: unknown command %q\n", os.Args[1])
		os.Exit(2)
	}
}

func spansFor(path string) FileSpans {
	res := FileSpans{Path: path}
	src, err := os.ReadFile(path)
	if err != nil {
		res.Error = err.Error()
		return res
	}
	fset := token.NewFileSet()
	// ParseComments so a declaration's doc comment is deleted with it. Leaving
	// the comment behind is not merely untidy: godox, gocritic's commentFormatting
	// and revive's comment rules all fire on comments, so an orphaned one keeps
	// producing the finding whose declaration the reducer just proved irrelevant.
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

	c := &collector{src: src, tf: tf}
	c.walkFile(f)
	res.Spans = c.spans
	return res
}

type collector struct {
	src   []byte
	tf    *token.File
	spans []Span
}

// off converts a token.Pos to a byte offset, clamped to the file. Clamping
// matters for `End()` of the last declaration, which can be one past the end.
func (c *collector) off(p token.Pos) int {
	if !p.IsValid() {
		return -1
	}
	o := c.tf.Offset(p)
	if o < 0 {
		return 0
	}
	if o > len(c.src) {
		return len(c.src)
	}
	return o
}

// widen grows [start,end) to whole lines when the bytes around it on those
// lines are blank. Deleting `\t\tfoo()` but not its indentation or its newline
// leaves a blank, indented line; a few hundred of those turn a reduced file
// into something a human still has to clean up before it can be a fixture.
func (c *collector) widen(start, end int) (int, int) {
	ls := start
	for ls > 0 && c.src[ls-1] != '\n' {
		ls--
	}
	if strings.TrimSpace(string(c.src[ls:start])) == "" {
		start = ls
	}
	le := end
	for le < len(c.src) && c.src[le] != '\n' {
		le++
	}
	if strings.TrimSpace(string(c.src[end:le])) == "" {
		if le < len(c.src) {
			le++ // take the newline too
		}
		end = le
	}
	return start, end
}

func (c *collector) add(kind string, start, end token.Pos, label string) {
	s, e := c.off(start), c.off(end)
	if s < 0 || e < 0 || e <= s {
		return
	}
	s, e = c.widen(s, e)
	c.spans = append(c.spans, Span{Kind: kind, Start: s, End: e, Label: label})
}

// addComma is add() for members of a comma-separated list. The element's own
// End() stops before its separator, so deleting exactly that span turns
// `{a: 1, b: 2}` into `{, b: 2}` — a file that no longer parses, which the
// reducer would then spend a test run rejecting for every such element.
func (c *collector) addComma(kind string, start, end token.Pos, label string) {
	s, e := c.off(start), c.off(end)
	if s < 0 || e < 0 || e <= s {
		return
	}
	j := e
	for j < len(c.src) && (c.src[j] == ' ' || c.src[j] == '\t') {
		j++
	}
	if j < len(c.src) && c.src[j] == ',' {
		e = j + 1
	}
	s, e = c.widen(s, e)
	c.spans = append(c.spans, Span{Kind: kind, Start: s, End: e, Label: label})
}

// declStart returns the position a declaration's deletion should begin at:
// its doc comment, when it has one.
func declStart(doc *ast.CommentGroup, fallback token.Pos) token.Pos {
	if doc != nil && doc.Pos().IsValid() {
		return doc.Pos()
	}
	return fallback
}

func (c *collector) walkFile(f *ast.File) {
	for _, d := range f.Decls {
		switch d := d.(type) {
		case *ast.FuncDecl:
			c.add("decl", declStart(d.Doc, d.Pos()), d.End(), funcLabel(d))
			c.addBody(d)
		case *ast.GenDecl:
			c.addGenDecl(d)
		}
	}
	// Statements and composite-type members live inside the declarations above,
	// so they are collected by a separate walk rather than inline: the reducer
	// needs the containing declaration's span too, and prefers it (one edit
	// beats twenty).
	ast.Inspect(f, func(n ast.Node) bool {
		switch n := n.(type) {
		case *ast.BlockStmt:
			c.addStmts(n.List)
		case *ast.CaseClause:
			c.addStmts(n.Body)
		case *ast.CommClause:
			c.addStmts(n.Body)
		case *ast.InterfaceType:
			// One method (or one embedded interface) at a time. This is the
			// span that isolates "which member of this interface does guff
			// disagree about" without rewriting the interface by hand.
			if n.Methods != nil {
				for _, m := range n.Methods.List {
					c.add("imethod", declStart(m.Doc, m.Pos()), m.End(), fieldLabel(m))
				}
			}
		case *ast.StructType:
			if n.Fields != nil {
				for _, fl := range n.Fields.List {
					c.add("field", declStart(fl.Doc, fl.Pos()), fl.End(), fieldLabel(fl))
				}
			}
		case *ast.CompositeLit:
			// One element of a struct/slice/map literal. Without this the
			// reducer cannot drop a field's initializer, so it cannot drop the
			// field, so it cannot drop the import the initializer needed — a
			// single `ready: btree.New(32, less[T])` pins three declarations
			// and a dependency. Elements are removable one at a time only for
			// keyed literals; a positional struct literal changes meaning if an
			// element goes missing, and the invariant would reject it anyway,
			// so the span is emitted and left for the oracle to judge.
			for _, elt := range n.Elts {
				c.addComma("elt", elt.Pos(), elt.End(), eltLabel(elt))
			}
		}
		return true
	})
}

func (c *collector) addStmts(list []ast.Stmt) {
	for _, s := range list {
		c.add("stmt", s.Pos(), s.End(), "")
	}
}

// addBody emits the body-to-panic rewrite described in the package comment.
//
// `panic(...)` rather than an empty body because an empty body is only legal
// for a function with no results, and the functions worth blanking are the ones
// with signatures other code depends on. The literal keeps the file compiling
// whatever the signature says.
func (c *collector) addBody(d *ast.FuncDecl) {
	if d.Body == nil || len(d.Body.List) == 0 {
		return
	}
	s, e := c.off(d.Body.Lbrace), c.off(d.Body.End())
	if s < 0 || e <= s {
		return
	}
	replacement := "{ panic(\"reduce\") }"
	if e-s <= len(replacement) {
		return // already at least as small
	}
	c.spans = append(c.spans, Span{
		Kind: "body", Start: s, End: e, Replace: replacement, Label: funcLabel(d),
	})
}

func (c *collector) addGenDecl(d *ast.GenDecl) {
	// The whole declaration, doc comment included.
	kind := "decl"
	if d.Tok == token.IMPORT {
		kind = "importdecl"
	}
	c.add(kind, declStart(d.Doc, d.Pos()), d.End(), d.Tok.String())

	// And each spec on its own, for grouped declarations. Ungrouped ones are
	// skipped: their single spec's span duplicates the declaration's, and a
	// duplicate span costs the reducer a test run to discover it changes nothing.
	if !d.Lparen.IsValid() {
		return
	}
	for _, spec := range d.Specs {
		switch s := spec.(type) {
		case *ast.ImportSpec:
			// Labelled with the import path, unquoted. The reducer looks the
			// span up by path when the compiler says "imported and not used":
			// deciding that question here would mean resolving package names,
			// which needs the build list. The compiler already knows.
			path, err := strconv.Unquote(s.Path.Value)
			if err != nil {
				path = s.Path.Value
			}
			c.add("import", declStart(s.Doc, s.Pos()), s.End(), path)
		case *ast.ValueSpec:
			c.add("spec", declStart(s.Doc, s.Pos()), s.End(), identsLabel(s.Names))
		case *ast.TypeSpec:
			c.add("spec", declStart(s.Doc, s.Pos()), s.End(), s.Name.Name)
		}
	}
}

func funcLabel(d *ast.FuncDecl) string {
	if d.Recv != nil && len(d.Recv.List) > 0 {
		return "(recv)." + d.Name.Name
	}
	return d.Name.Name
}

func fieldLabel(f *ast.Field) string {
	if len(f.Names) > 0 {
		return identsLabel(f.Names)
	}
	if id, ok := f.Type.(*ast.Ident); ok {
		return id.Name // embedded
	}
	if sel, ok := f.Type.(*ast.SelectorExpr); ok {
		if x, ok := sel.X.(*ast.Ident); ok {
			return x.Name + "." + sel.Sel.Name
		}
	}
	return ""
}

// eltLabel names a composite-literal element by its key, when it has one.
func eltLabel(e ast.Expr) string {
	kv, ok := e.(*ast.KeyValueExpr)
	if !ok {
		return ""
	}
	if id, ok := kv.Key.(*ast.Ident); ok {
		return id.Name
	}
	return ""
}

func identsLabel(names []*ast.Ident) string {
	parts := make([]string, 0, len(names))
	for _, n := range names {
		parts = append(parts, n.Name)
	}
	return strings.Join(parts, ",")
}
