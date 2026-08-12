// Package parens pins how each linter treats a redundant parenthesis and a
// stray comment.
//
// Every shape here came out of `compat/fuzz.py` (COMPAT-HARDENING Phase 6),
// which mutates a fixture and asks whether the two tools still agree. Wrapping
// an expression in `()` or putting a comment on a line changes nothing about
// what the program does, and changes a great deal about what upstream reports —
// differently per linter, because each one navigates the AST its own way:
//
//	revive        plain type assertions (`arg.(*ast.CallExpr)`)  -> never unwraps
//	honnef S1008  pattern.match, which unwraps ParenExpr on both sides
//	honnef QF1003 astutil.Equal, which opens with reflect.TypeOf -> never unwraps
//	govet assign  reflect.TypeOf(lhs) == reflect.TypeOf(rhs)     -> never unwraps
//
// guff got this wrong in both directions at once — unwrapping where revive and
// QF1003 do not, and not unwrapping where S1008 does. Each function below is
// paired with an unparenthesized control so the golden records both that the
// paren form is silent and that the plain form still fires.
//
// The module is `go 1.21` so revive's range-val-address applies at all; it
// returns early for go1.22+, where each iteration has its own variable.
//
// DO NOT gofmt this file. Every parenthesis in it is the thing under test, and
// `gofmt` deletes the one in `S1008ParenCond`'s `if (b == true)` — that is the
// only redundant paren gofmt removes, and removing it removes the case. (212
// other files under crates/*/tests/testdata are unformatted for similar
// reasons; no gate formats them.)
package parens

import (
	"errors"
	"fmt"
)

type S struct{ f int8 }

// --- govet assign -----------------------------------------------------------

// Upstream short-circuits on `reflect.TypeOf(lhs) != reflect.TypeOf(rhs)`
// before the rendered-source comparison, and the comparison itself goes through
// go/printer, which would erase the parens. Ident vs ParenExpr: silent.
func AssignParen(x int) int {
	x = (x)
	return x
}

// The control: reported.
func AssignPlain(x int) int {
	x = x
	return x
}

// Reported, and the operand is named with the rendered source (`s.f`), not with
// an identifier — guff used to print `_` for anything that was not a bare name.
func AssignSelector(s *S) {
	s.f = s.f
}

// Silent: upstream requires NoEffects on both sides, and deleting this
// statement would delete two calls.
func AssignEffects(a []int, f func() int) {
	a[f()] = a[f()]
}

// --- govet shift ------------------------------------------------------------

// All three are reported, and all three name the operand with go/printer.
// guff used the literal string "x" for every operand that was not an
// identifier, so these three produced the same message.
func ShiftSelector(s S) int8   { return s.f << 10 }
func ShiftParen(i int8) int8   { return (i) << 10 }
func ShiftIndex(a []int8) int8 { return a[0] << 10 }

// --- staticcheck S1008 ------------------------------------------------------

// honnef's pattern matcher unwraps ParenExpr, so both of these still match and
// the message renders the *unwrapped* condition.
func S1008ParenCond(b bool) bool {
	if (b == true) {
		return true
	}
	return false
}

func S1008ParenReturn(x int) bool {
	if x == 1 {
		return (true)
	}
	return false
}

// A comment associated with either branch silences it: upstream builds an
// ast.CommentMap and bails. guff had the guard but its analysis AST carries no
// comments below the file header, so the guard always answered "no".
func S1008CommentBefore(x int) bool {
	if x == 1 {
		return true
	}
	// this comment is the whole point
	return false
}

func S1008CommentInside(x int) bool {
	if x == 1 {
		// and so is this one
		return true
	}
	return false
}

// The control: no parens, no comments, reported.
func S1008Plain(x int) bool {
	if x == 1 {
		return true
	}
	return false
}

// --- staticcheck QF1003 -----------------------------------------------------

// astutil.Equal compares node types first, so `(x)` is not equal to `x` and the
// if/else chain is not a tagged switch. Silent.
func QF1003Paren(x int) string {
	if x == 1 {
		return "one"
	} else if (x) == 2 {
		return "two"
	}
	return "other"
}

// The control: reported.
func QF1003Plain(x int) string {
	if x == 1 {
		return "one"
	} else if x == 2 {
		return "two"
	}
	return "other"
}

// --- revive errorf ----------------------------------------------------------

// Upstream asserts `arg.(*ast.CallExpr)` with no unwrapping: silent.
func ErrorfParen(name string) error {
	return errors.New((fmt.Sprintf("bad %s", name)))
}

// The control: reported.
func ErrorfPlain(name string) error {
	return errors.New(fmt.Sprintf("bad %s", name))
}

// --- revive range-val-address -----------------------------------------------

// `switch e := exp.(type)` over the RHS matches neither UnaryExpr nor CallExpr
// when a ParenExpr is in the way: silent.
func RangeAddrParen() []*int {
	var out []*int
	for _, v := range []int{1, 2, 3} {
		out = (append(out, &v))
	}
	return out
}

// The control: reported.
func RangeAddrPlain() []*int {
	var out []*int
	for _, v := range []int{1, 2, 3} {
		out = append(out, &v)
	}
	return out
}
