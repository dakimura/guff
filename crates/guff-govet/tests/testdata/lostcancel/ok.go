package p

import "context"

func f() {
	_, cancel := context.WithCancel(context.Background())
	defer cancel()
}

type store struct{ shutdown context.CancelFunc }

// go/cfg's builder treats **each `ValueSpec` as its own statement**, so the
// remainder of the defining block that upstream scans for a use includes the
// later specs of the same `var (…)` group. Walking the statement list instead
// — where the whole group is one element — starts after it and misses this.
// flipt's `internal/storage/authn/memory.NewStore` writes exactly this.
func siblingSpecUsesCancel() *store {
	var (
		_, cancel = context.WithCancel(context.Background())
		s         = &store{shutdown: cancel}
	)
	return s
}

// The same use, one spec later still.
func twoSpecsLater() *store {
	var (
		_, cancel = context.WithCancel(context.Background())
		_         = 1
		s         = &store{shutdown: cancel}
	)
	return s
}

// The separate-statement spelling, which always worked — the control that says
// the fix is about the group, not about struct literals.
func separateStatements() *store {
	_, cancel := context.WithCancel(context.Background())
	s := &store{shutdown: cancel}
	return s
}
