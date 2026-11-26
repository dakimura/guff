//go:build go1.24

// `testingcontext` replaces
//
//	ctx, cancel := context.WithCancel(context.Background())
//	defer cancel()
//
// with `ctx := t.Context()`, and only inside a test function. Upstream keys off
// the *call* and walks out to the enclosing function; guff walked down from
// each declaration into every literal with a `*testing.T` parameter, which both
// invented findings (vitess wraps its subtests in `synctest.Test`) and missed
// them (a pair inside a plain block, a `for` body or a `case` clause).
//
// Every shape below was measured against golangci-lint 2.12.2.
package modernize

import (
	"context"
	"testing"
	"testing/synctest"
)

// Reported: the plain test function.
func TestPlain(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	_ = ctx
}

// Reported: a `t.Run` subtest — the literal is argument index 1 of `T.Run`.
func TestSubtest(t *testing.T) {
	t.Run("x", func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	})
}

// Silent: same shape, same argument index, but the callee is not `Run`.
func TestSynctest(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	})
}

// Silent: `_` declares no context to rename.
func TestBlankCtx(t *testing.T) {
	_, cancel := context.WithCancel(context.Background())
	defer cancel()
}

func helper(t *testing.T, f func(*testing.T)) { f(t) }

// Silent: a helper of the package's own is not `Run` either.
func TestHelper(t *testing.T) {
	helper(t, func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	})
}

func helper0(f func(*testing.T), t *testing.T) { f(t) }

// Silent: the literal is argument index 0.
func TestArgZero(t *testing.T) {
	helper0(func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	}, t)
}

// Silent: `cancel` is used somewhere other than the deferred call.
func TestCancelUsedTwice(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	cancel()
	_ = ctx
}

// Silent: the parent is neither `Background` nor `TODO`.
func TestNotBackground(t *testing.T) {
	base := context.Background()
	ctx, cancel := context.WithCancel(base)
	defer cancel()
	_ = ctx
}

// Reported: `B.Run` counts as well as `T.Run`, and the message names `b`.
func BenchmarkSub(b *testing.B) {
	b.Run("x", func(b *testing.B) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	})
}

// Silent: the subtest parameter has no name.
func TestUnnamedParam(t *testing.T) {
	t.Run("x", func(*testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	})
}

// Reported: `context.TODO` is the other accepted parent.
func TestTODO(t *testing.T) {
	ctx, cancel := context.WithCancel(context.TODO())
	defer cancel()
	_ = ctx
}

// Silent: `t` no longer means the test where the fix would be written.
func TestShadowed(t *testing.T) {
	{
		t := 1
		_ = t
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	}
}

// Reported: a bare block is still inside the test function.
func TestNestedBlock(t *testing.T) {
	{
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	}
}

// Reported: so is a `for` body.
func TestForBody(t *testing.T) {
	for range 1 {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	}
}

// Reported: and a `case` clause.
func TestSwitchCase(t *testing.T) {
	switch 1 {
	case 1:
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	}
}

// Reported: the message names whatever the parameter is called.
func TestRenamedParam(t *testing.T) {
	t.Run("x", func(sub *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	})
}

// Silent: the deferred call is not the next statement.
func TestNotAdjacent(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	defer cancel()
}
