package p

import (
	"errors"
	"fmt"
)

// err113 has two messages with two different positions: the definition check
// reports the CallExpr, the comparison check reports the BinaryExpr and carries
// a suggested fix.

// "do not define dynamic errors, use wrapped static errors instead: …"
func Define() error {
	return errors.New("dynamic")
}

// The definition message renders the whole call with `go/printer`, so an
// argument that is itself an expression pins that it is not an approximation.
func DefineFormatted(name string, n int) error {
	return fmt.Errorf("%s: %d", name, n*2+1)
}

var ErrSentinel = errors.New("sentinel")

type box struct{ err error }

func wrap(ctx string, e error) error { return fmt.Errorf("%s: %w", ctx, e) }

// "do not compare errors directly …, use … instead" — both directions of the
// comparison, since the message renders the operator it saw.
func CompareEqual(err error) bool {
	return err == ErrSentinel
}

func CompareNotEqual(err error) bool {
	return err != ErrSentinel
}

// The two halves of the message do NOT share a renderer: the quoted original is
// `render(fset, be)` (go/printer), but the suggestion is built from
// `rawString`, a hand-rolled walker whose CallExpr arm is `fmt.Sprintf("%s()",
// …)` — it drops every argument. So this one comparison has to print the same
// call two different ways.
func CompareCall(err error, ctx string) bool {
	return err != wrap(ctx, ErrSentinel)
}

// `rawString` handles only Ident, SelectorExpr and CallExpr; everything else
// falls through to `fmt.Sprintf("%s", x)` on the *ast node, which prints a Go
// struct dump with absolute `token.Pos` integers in it. Deliberately not
// fixtured: those integers depend on the whole FileSet layout, so the line is
// neither reproducible as a golden nor meaningful to match.

func CompareField(err error, b box) bool {
	return err != b.err
}
