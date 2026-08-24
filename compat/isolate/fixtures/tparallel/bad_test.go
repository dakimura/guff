package p

import "testing"

// tparallel has three messages and the fixture reached one.

// "%s's subtests should call t.Parallel"
func TestSubtestsMissing(t *testing.T) {
	t.Parallel()
	t.Run("sub", func(t *testing.T) {
		_ = 1
	})
}

// "%s should call t.Parallel on the top level as well as its subtests"
func TestTopLevelMissing(t *testing.T) {
	t.Run("sub", func(t *testing.T) {
		t.Parallel()
		_ = 1
	})
}

// "%s should use t.Cleanup instead of defer"
func TestDeferNotCleanup(t *testing.T) {
	t.Parallel()
	defer func() { _ = 1 }()
	t.Run("sub", func(t *testing.T) {
		t.Parallel()
		_ = 1
	})
}
