package p

import (
	"testing"
	"testing/synctest"
)

func helper(t *testing.T) {
	t.Fatal("x")
}

func TestBad(t *testing.T) {
	helper(t)
}

// `synctest.Test(t, func(*testing.T))` hands its literal a fresh `*testing.T`
// the same way `t.Run` does, and upstream filters it out of the report set. It
// is not a helper and must not be asked for `t.Helper()`.
func TestSynctest(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		t.Log("in the bubble")
	})
}

// A `Test` method on something that is not the synctest package is not this,
// and its literal keeps the finding.
type notSynctest struct{}

func (notSynctest) Test(t *testing.T, f func(*testing.T)) { f(t) }

func TestLookalike(t *testing.T) {
	var synctest notSynctest
	synctest.Test(t, func(t *testing.T) {
		t.Log("not in a bubble")
	})
}
