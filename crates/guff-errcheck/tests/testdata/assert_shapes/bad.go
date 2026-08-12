// Package assertshapes covers *where* an unchecked type assertion is reported
// and, just as much, where upstream stops looking.
//
// kisielk's visitor prunes: `case *ast.TypeAssertExpr` returns nil, and so does
// an assignment whose single RHS is an assertion. The pruned subtree is not
// examined at all, so an unchecked call inside a function literal in there is
// not reported either. Neither type switch unwraps parentheses, so `(f())` on
// the right of `=` is not a blank assignment and `(i.(string))` is reported by
// the assertion's own visit — one column further right than the bare form.
package assertshapes

func f() error           { return nil }
func b() (string, error) { return "", nil }
func sink(string)        {}

func Positions(i any) {
	// `checkAssignment` reports at the RHS node's start, and an assertion
	// node starts at its operand.
	var a = i.(string)
	_ = a

	// Two names on the left: the result *is* read, so nothing is reported —
	// and the assertion's own visit does not report it either, because the
	// assignment pruned it.
	var s, ok = i.(string)
	_, _ = s, ok

	// `checkAssertExpr` reports at `expr.Pos()`, which is the operand, not
	// `.(`. Every one of these is a column the `.(`-based reading gets wrong.
	sink(i.(string))
	if i.(string) == "" {
	}
	_ = []string{i.(string)}
}

func Pruning(i any) {
	// Nested: only the outer assertion is visited, and both would report the
	// same position anyway — the difference is that the inner one is never
	// examined.
	_ = i.(any).(string)

	// The pruned subtree contains an unchecked call. Upstream never sees it.
	_ = (func() error { f(); return nil })().(error)
}

func Parens(i any) {
	// A `*ast.ParenExpr` matches neither arm of `checkAssignment`: this is
	// not a blank assignment, whatever `check-blank` says.
	_ = (f())
	var _ = (f())

	// Same fallthrough, but the assertion is then reported by its own visit,
	// at the operand inside the parentheses.
	_ = (i.(string))
}

func Multi(i any, j any) {
	// The multi-value arm reports an assertion for *any* name on the left,
	// not only for `_` — unlike the call arm right below it.
	a, c := i.(string), j.(int)
	_, _ = a, c

	_, err := b()
	_ = err
}
