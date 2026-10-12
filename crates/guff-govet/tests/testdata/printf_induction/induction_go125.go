//go:build go1.25

package ind

// Declared in a go1.25 file: no induction.
type IOld interface{ Logf(format string, args ...any) }

var _ IOld = impl(0)

func _(l IOld) { l.Logf("%s", 99) }
