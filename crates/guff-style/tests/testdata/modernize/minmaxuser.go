// minmax's *other* arm: a package-level `func min`/`func max` that the
// built-in already does.
//
//	if fn, ok := pass.Pkg.Scope().Lookup(funcName).(*types.Func); ok {
//
// The lookup is in the **package scope**, so a method or a local named `min`
// is not it, and the report covers the whole declaration. guff had only the
// if/else arm; beats writes three of these.
//
// `hasMinMaxLogic` accepts two bodies — one `if/else`, or an `if` followed by
// a `return` — and `checkMinMaxPattern` then requires the two returned
// expressions to be the comparison's own operands, in either order, with the
// direction spelling the function's name.
package minmax

// Pattern 1: one if/else.
func min(a, b int) int {
	if a < b {
		return a
	} else {
		return b
	}
}

// Pattern 2: an if, then a return.
func max(a, b int64) int64 {
	if a > b {
		return a
	}
	return b
}

type holder struct{}

// Silent: a method is not in the package scope.
func (holder) min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// Silent: "only the most common case: exactly 2 parameters".
func min3(a, b, c int) int {
	if a < b {
		return a
	}
	return b
}

// Silent: the body returns the wrong operand, so the direction spells `max`
// while the name says `min`.
func notMin(a, b int) int {
	if a < b {
		return b
	}
	return a
}

var (
	_ = min(1, 2)
	_ = max(int64(1), int64(2))
)
