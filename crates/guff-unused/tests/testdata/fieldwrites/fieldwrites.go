// Package fieldwrites is the `linters.settings.unused` half of unused.
//
// `field-writes-are-uses` (upstream default true) decides whether writing to a
// struct field keeps it alive. With it off, `g.write` on a selector stops
// reading the selector: `g.read(node.X, by); g.write(node.Sel, by)`, and
// `g.write` of an `*ast.Ident` does nothing outside tests. `post-statements-
// are-reads` (default false) decides the same question for `x.n++`.
//
// Every shape below was measured against golangci-lint 2.12.2 under three
// configs — both defaults, `field-writes-are-uses: false`, and that plus
// `post-statements-are-reads: true` — and the three tests pin the three
// answers.
package fieldwrites

type keyedOnly struct {
	Exported string
	written  string
}

func UseKeyed() string {
	v := keyedOnly{Exported: "a", written: "b"}
	return v.Exported
}

type unkeyedOnly struct {
	Exported string
	written  string
}

func UseUnkeyed() string {
	v := unkeyedOnly{"a", "b"}
	return v.Exported
}

type assignedOnly struct {
	Exported string
	written  string
}

func UseAssigned() string {
	var v assignedOnly
	v.written = "b"
	return v.Exported
}

// Written *and* read: the read is a use whatever the option says.
type readBack struct {
	Exported string
	written  string
}

func UseRead() string {
	var v readBack
	v.written = "b"
	return v.Exported + v.written
}

// `g.write(stmt.X)` only, unless `post-statements-are-reads`.
type incremented struct {
	Exported string
	n        int
}

func UseIncr() string {
	var v incremented
	v.n++
	return v.Exported
}

// The assignment token is never examined: `+=` is as much a pure write as `=`.
type opAssign struct {
	Exported string
	written  string
}

func UseOpAssign() string {
	var v opAssign
	v.written += "b"
	return v.Exported
}

// The same field on the right-hand side is an ordinary read.
type selfRead struct {
	Exported string
	written  string
}

func UseSelfRead() string {
	var v selfRead
	v.written = v.written + "b"
	return v.Exported
}

// `v.a.b = "x"` reads `v.a` — the walk reaches that selector on its own — and
// writes `b`.
type inner struct{ b string }

type outer struct {
	Exported string
	a        inner
}

func UseNested() string {
	var v outer
	v.a.b = "x"
	return v.Exported
}

// Taking a field's address is a read.
type addrOf struct {
	Exported string
	written  string
}

func UseAddrOf() string {
	var v addrOf
	p := &v.written
	_ = p
	return v.Exported
}

// Every LHS of a multi-assignment is written, not just the first.
type multiAssign struct {
	Exported string
	written  string
}

func UseMulti() string {
	var v multiAssign
	v.written, v.Exported = "a", "b"
	return v.Exported
}

// A *promoted* write reads only `v`, so neither the embedded field nor the
// field it promotes is used. `hidden` is not reported because its owner type
// is the finding and `colorAndQuieten` silences what it owns.
type pin struct{ hidden string }

type pout struct {
	Exported string
	pin
}

func UsePromotedWrite() string {
	var v pout
	v.hidden = "x"
	return v.Exported
}

// `g.write` recurses through `*ast.ParenExpr` only; the star below is inside
// the selector's `X`, so the selector is still the write.
type derefd struct {
	Exported string
	written  string
}

func UseDeref() string {
	v := &derefd{}
	(*v).written = "x"
	return v.Exported
}

// `g.write(stmt.Key)` — a range clause assigns like an assignment does.
type ranged struct {
	Exported string
	written  int
}

func UseRange(xs []string) string {
	var v ranged
	for v.written = range xs {
	}
	return v.Exported
}

// `g.write` of an `*ast.IndexExpr` *reads* the operand and stops, so indexing
// into a field is a use of the field.
type indexed struct {
	Exported string
	written  []string
}

func UseIndexed() string {
	var v indexed
	v.written[0] = "x"
	return v.Exported
}
