// The NaN exemption, which needs a package of its own: only one `min` and one
// `max` can be declared per package.
//
// `maybeNaN` takes the parameter's core type and refuses on a float, because
// `min(NaN, x)` and the hand-written comparison disagree.
package minmax

// Silent: float parameters.
func min(a, b float64) float64 {
	if a < b {
		return a
	}
	return b
}

// Reported: the comparison's operands are returned in the *other* order, which
// flips the sign back to `max`.
func max(a, b int) int {
	if b > a {
		return b
	}
	return a
}

var (
	_ = min(1.0, 2.0)
	_ = max(1, 2)
)
