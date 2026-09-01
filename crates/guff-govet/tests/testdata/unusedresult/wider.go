package bad

import (
	stderrors "errors"
	"fmt"
	"maps"
	"slices"
	"sort"
)

// The list is keyed on the callee's **package path** and name — what
// `fn.Pkg().Path()` answers — not on the identifier written before the dot.
// Matching the qualifier instead read `github.com/pkg/errors.New` as
// `errors.New` and reported it, and missed the real one imported under a name
// (velero, both halves of the same defect).
func aliasedImport() {
	stderrors.New("x")
}

// Most of the list is `fmt.Append*`, `maps.*` and `slices.*`, which guff's
// hand-written table did not have at all.
func listEntries(m map[string]int, xs []int) {
	fmt.Append(nil, "x")
	fmt.Appendf(nil, "%d", 1)
	fmt.Appendln(nil, "x")
	maps.Keys(m)
	maps.Clone(m)
	slices.Clone(xs)
	slices.Contains(xs, 1)
	sort.Reverse(sort.IntSlice(xs))
}

type stringer struct{ s string }

func (s stringer) String() string { return s.s }

func (s stringer) Error() string { return s.s }

// Named like one on the list, but not `func() string`.
func (s stringer) Errorf(a int) string { return s.s }

// `func() string`, but not named like one on the list.
func (s stringer) Describe() string { return s.s }

// The second list is of *methods*: `Error` and `String`, and only when the
// signature is identical to `func() string`. guff had no method branch at all.
// The message names the receiver type, written with a nil qualifier — so the
// package path, and `error` for the interface.
func stringMethods(s stringer, err error) {
	s.String()
	s.Error()
	err.Error()
	s.Describe()
	s.Errorf(1)
}

var sprintf = fmt.Sprintf

// A call through a variable of function type resolves to no `types.Func`.
func throughVar() {
	sprintf("x")
}
