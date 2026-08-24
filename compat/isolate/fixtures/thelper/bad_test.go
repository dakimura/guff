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

// thelper says three different things and checks four different subjects
// (test / benchmark / tb / fuzz). The three messages below are the whole
// vocabulary; a fixture with one `*testing.T` helper reaches one of them.

// "test helper function should start from b.Helper()" — the benchmark subject.
func benchHelper(b *testing.B) {
	b.Fatal("x")
}

func BenchmarkMissingHelper(b *testing.B) {
	benchHelper(b)
}

// "parameter *testing.T should have name t" — right type, wrong name.
func wrongName(x *testing.T) {
	x.Helper()
	x.Fatal("x")
}

func TestWrongParamName(t *testing.T) {
	wrongName(t)
}

// "parameter *testing.T should be the first or after context.Context".
func wrongPosition(msg string, t *testing.T) {
	t.Helper()
	t.Fatal(msg)
}

func TestWrongParamPosition(t *testing.T) {
	wrongPosition("x", t)
}

// The `testing.TB` subject is checked separately from `*testing.T`.
func tbHelper(tb testing.TB) {
	tb.Fatal("x")
}

func TestTBHelper(t *testing.T) {
	tbHelper(t)
}
