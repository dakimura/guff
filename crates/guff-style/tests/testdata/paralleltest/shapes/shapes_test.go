// Package shapes holds the paralleltest branches the upstream testdata does
// not reach: which loop variable name is reported, builders called through a
// selector or with an unnamed parameter, and Chdir on a non-testing value.
package shapes

import (
	"fmt"
	"testing"
)

type suite struct{}

func (suite) builder() func(*testing.T) {
	return func(t *testing.T) {
		t.Parallel()
	}
}

func unnamedBuilder() func(*testing.T) {
	return func(*testing.T) {}
}

func serialBuilder() func(*testing.T) {
	return func(t *testing.T) {
		fmt.Println("serial")
	}
}

func nestedReturnBuilder() func(*testing.T) {
	inner := func() func(*testing.T) {
		return func(tt *testing.T) {
			tt.Parallel()
		}
	}
	return inner()
}

// Both loop variables are used; upstream keeps the last identifier it walks.
func TestKeyAndValue(t *testing.T) {
	t.Parallel()
	for i, tc := range []string{"a", "b"} {
		t.Run(tc, func(t *testing.T) {
			t.Parallel()
			fmt.Println(tc, i)
		})
	}
}

// Only the key.
func TestKeyOnly(t *testing.T) {
	t.Parallel()
	for i := range []string{"a"} {
		t.Run("x", func(t *testing.T) {
			t.Parallel()
			fmt.Println(i)
		})
	}
}

// Reinitialised: the closure uses the copy, not the loop variable.
func TestReinitialised(t *testing.T) {
	t.Parallel()
	for _, tc := range []string{"a"} {
		tc := tc
		t.Run(tc, func(t *testing.T) {
			t.Parallel()
			fmt.Println(tc)
		})
	}
}

// Loop variable only in the name argument, not the callback.
func TestLoopVarOnlyInName(t *testing.T) {
	t.Parallel()
	for _, tc := range []string{"a"} {
		t.Run(tc, func(t *testing.T) {
			t.Parallel()
		})
	}
}

// Setenv blocks parallel, but the loop variable is still captured.
func TestBlockedButCaptured(t *testing.T) {
	for _, tc := range []string{"a"} {
		t.Run(tc, func(t *testing.T) {
			t.Setenv("K", tc)
		})
	}
}

// Missing parallel in the range: the reinitialise message is not reported.
func TestMissingAndCaptured(t *testing.T) {
	t.Parallel()
	for _, tc := range []string{"a"} {
		t.Run(tc, func(t *testing.T) {
			fmt.Println(tc)
		})
	}
}

// Two range statements: only the first capture is remembered.
func TestTwoRanges(t *testing.T) {
	t.Parallel()
	for _, first := range []string{"a"} {
		t.Run("1", func(t *testing.T) {
			t.Parallel()
			fmt.Println(first)
		})
	}
	for _, second := range []string{"b"} {
		t.Run("2", func(t *testing.T) {
			t.Parallel()
			fmt.Println(second)
		})
	}
}

var table = []func(*testing.T){serialBuilder()}

func TestBuilders(t *testing.T) {
	t.Parallel()
	var s suite
	t.Run("selector", s.builder())
	t.Run("unnamed", unnamedBuilder())
	t.Run("serial", serialBuilder())
	t.Run("nested", nestedReturnBuilder())
	t.Run("indexed", table[0])
}

type dir struct{}

func (dir) Chdir(string) {}

// Matched by receiver name: `t` here is not a *testing.T.
func TestChdirByName(q *testing.T) {
	t := dir{}
	t.Chdir(".")
}

func TestChdirInRange(t *testing.T) {
	t.Parallel()
	for _, d := range []string{"a"} {
		t.Run(d, func(t *testing.T) {
			t.Chdir(d)
		})
	}
}
