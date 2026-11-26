// Interface shapes refactor/satisfy and printf's induction must agree on with
// upstream: anonymous interfaces, a type-parameter map key, a method expression
// (no constraint), an embedded interface field, a forwarding cycle, and the
// print and errorf kinds.
package shapes

import "fmt"

type impl int

func (impl) Logf(format string, args ...any) { print(fmt.Sprintf(format, args...)) }

// Anonymous interface in a parameter.
func takeAnon(l interface{ Logf(format string, args ...any) }) {
	l.Logf("%s", 1)
}

func _() { takeAnon(impl(0)) }

// Anonymous interface in a struct field.
type box struct {
	l interface{ Logf(format string, args ...any) }
}

func _(b box) { b.l.Logf("%s", 2) }

var _ = box{l: impl(0)}

// A generic map keyed by a type parameter's core type.
type IGenMap interface{ Logf(format string, args ...any) }

func lookup[M ~map[IGenMap]int](m M) int { return m[impl(0)] }
func _(l IGenMap)                         { l.Logf("%s", 3) }

// Method expression on an interface.
type IMexpr interface{ Logf(format string, args ...any) }

var _ = IMexpr.Logf

func _(l IMexpr) { l.Logf("%s", 4) }

// An interface whose implementation is an embedded interface field.
type IField interface{ Logf(format string, args ...any) }
type viaIface struct{ IMexpr }

var _ IField = viaIface{}

func _(l IField) { l.Logf("%s", 5) }

// Implementation forwards to the interface it implements (a cycle).
type ICycle interface{ Logf(format string, args ...any) }
type cyc struct{ next ICycle }

func (c cyc) Logf(format string, args ...any) { c.next.Logf(format, args...) }

var _ ICycle = cyc{}

func _(l ICycle) { l.Logf("%s", 6) }

// Print-like.
type IPrintln interface{ Println(args ...any) }
type pl int

func (pl) Println(args ...any) { fmt.Println(args...) }

var _ IPrintln = pl(0)

func _(l IPrintln) { l.Println("%d", 7) }

// Errorf-like: implementation forwards to fmt.Errorf.
type IErr interface{ Errorf(format string, args ...any) error }
type er int

func (er) Errorf(format string, args ...any) error { return fmt.Errorf(format, args...) }

var _ IErr = er(0)

func _(l IErr) { _ = l.Errorf("%w", 8) }

// Comparison with a typed nil pointer.
type ICmpPtr interface{ Logf(format string, args ...any) }
type pimpl struct{}

func (*pimpl) Logf(format string, args ...any) { print(fmt.Sprintf(format, args...)) }

func _(l ICmpPtr) bool { return l != (*pimpl)(nil) }
func _(l ICmpPtr)      { l.Logf("%s", 9) }
