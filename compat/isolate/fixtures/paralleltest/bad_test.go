package p

import "testing"

// paralleltest has five messages. Four of them end with a literal `\n`, which
// only the golden tier can see — the tiers that normalize strip trailing
// whitespace.

// 1. "Function %s missing the call to method parallel\n", at funcDecl.Pos().
func TestMissingParallel(t *testing.T) {
	t.Run("sub", func(t *testing.T) {
		_ = 1
	})
}

// 4. "Function %s missing the call to method parallel in the test run\n", at the
//    t.Run callback's Pos() — one per run, so two subtests are two findings.
func TestRunsMissingParallel(t *testing.T) {
	t.Parallel()
	t.Run("one", func(t *testing.T) {
		_ = 1
	})
	t.Run("two", func(t *testing.T) {
		_ = 1
	})
}

// 2. "Range statement for test %s missing the call to method parallel in test
//    Run\n", at the range statement — the subtest inside the loop is the one
//    that has to call it.
func TestRangeMissingParallel(t *testing.T) {
	t.Parallel()
	cases := []struct{ name string }{{"a"}}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_ = tc
		})
	}
}

// 3. "Range statement for test %s does not reinitialise the variable %s\n" is
//    the fifth message and is **unreachable through golangci-lint on any
//    modern module**: its wrapper sets `ignoreloopVar = true` whenever the
//    effective Go version is >= 1.22, because loop variables are per-iteration
//    from then on. This function is the shape that would trigger it, kept as a
//    negative: upstream says nothing here, so guff must not either.
func TestRangeNoReinit(t *testing.T) {
	t.Parallel()
	cases := []struct{ name string }{{"a"}}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			_ = tc
		})
	}
}

// 5. "Function %s uses defer with t.Parallel, use t.Cleanup instead …" — the one
//    message with **no** trailing newline, and off unless `ignore-missing` peers
//    are configured: it needs `check-cleanup`.
func TestDeferWithParallel(t *testing.T) {
	t.Parallel()
	defer func() { _ = 1 }()
	_ = 1
}
