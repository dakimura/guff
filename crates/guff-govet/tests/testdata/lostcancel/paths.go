// Path shapes for lostcancel's "not used on all paths" arm.
//
// Every `leak*` function is reported twice by upstream (at the defining
// statement and at the return statement the search reaches); every `ok*`
// function is silent. The golden case `compat/golden/cases/govet`
// pins the positions and the wording against golangci-lint.
package p

import (
	"context"
	"time"
)

// --- clean ---------------------------------------------------------------

// A deferred cancel covers every return.
func okDeferred() error {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	_ = ctx
	return nil
}

// Both branches define their own cancel var and defer it. The `else if` branch
// is the shape that consul's leader_connect_ca.go and server.go use.
func okElseIf(b, c bool) error {
	if b {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = ctx
	} else if c {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = ctx
	}
	return nil
}

func okSwitchCase(n int) error {
	switch n {
	case 1:
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	}
	return nil
}

func okSelectCase(ch chan int) error {
	select {
	case <-ch:
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
	}
	return nil
}

// Handing cancel to a goroutine counts as a use.
func okGoroutine() error {
	ctx, cancel := context.WithCancel(context.Background())
	go func() { cancel() }()
	_ = ctx
	return nil
}

// Returning cancel to the caller counts as a use.
func okReturnsCancel() (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(context.Background())
	return ctx, cancel
}

func okLabeledLoop() error {
L:
	for {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		_ = ctx
		break L
	}
	return nil
}

// Every path through the if/else cancels, so the return below is covered.
func okBothBranches(b bool) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if b {
		cancel()
	} else {
		cancel()
	}
	return nil
}

// Same, via a switch that has a default.
func okSwitchDefault(n int) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	switch n {
	case 1:
		cancel()
	default:
		cancel()
	}
	return nil
}

// A reference in the condition runs on every path.
func okUsedInCondition(b bool) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if cancel != nil && b {
		_ = 1
	}
	return nil
}

// The loop is left only through its body, which cancels.
func okUnconditionalLoop() error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	for {
		cancel()
		break
	}
	return nil
}

// panic() ends the function, so there is no return to reach.
func okPanics(b bool) {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if b {
		cancel()
	}
	panic("x")
}

// A naked return counts as a use of the named results.
func okNamedResults() (ctx context.Context, cancel context.CancelFunc) {
	ctx, cancel = context.WithCancel(context.Background())
	return
}

// The variable outlives the function, so other uses may exist.
var pkgCancel context.CancelFunc

func okOuterScope(b bool) error {
	var ctx context.Context
	ctx, pkgCancel = context.WithCancel(context.Background())
	_ = ctx
	if b {
		pkgCancel()
	}
	return nil
}

// --- reported ------------------------------------------------------------

// The only use sits on one branch.
func leakConditionalUse(b bool) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if b {
		cancel()
	}
	return nil
}

// The cancel func is discarded outright (a single report, at the `_`).
func leakDiscarded() {
	ctx, _ := context.WithCancel(context.Background())
	_ = ctx
}

// An earlier return skips the cancel below it.
func leakEarlyReturn(b bool) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if b {
		return nil
	}
	cancel()
	return nil
}

// A differently named variable is named in both messages.
func leakNamedKill(b bool) error {
	ctx, kill := context.WithCancel(context.Background())
	_ = ctx
	if b {
		kill()
	}
	return nil
}

// A switch with no default can be skipped entirely.
func leakSwitchNoDefault(n int) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	switch n {
	case 1:
		cancel()
	}
	return nil
}

// Reported inside the loop body: the return there precedes the cancel.
func leakInLoopBody(b bool) error {
	for i := 0; i < 3; i++ {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		_ = ctx
		if b {
			return nil
		}
		cancel()
	}
	return nil
}

// Function literals are analyzed as functions of their own.
func leakInFuncLit(b bool) func() error {
	return func() error {
		ctx, cancel := context.WithCancel(context.Background())
		_ = ctx
		if b {
			cancel()
		}
		return nil
	}
}

// Storing cancel is a use, but only on the branch that stores it.
type holder struct{ c context.CancelFunc }

func leakStoredOnOnePath(b bool) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if b {
		_ = holder{c: cancel}
	}
	return nil
}

// No explicit return: the report lands on the closing brace, where upstream's
// CFG puts its synthetic return.
func leakFallsOffEnd(b bool) {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if b {
		cancel()
	}
}

// The `var` form reports at the spec, past the `var` keyword.
func leakVarDecl(b bool) error {
	var ctx, cancel = context.WithCancel(context.Background())
	_ = ctx
	if b {
		cancel()
	}
	return nil
}

// One branch hands cancel to the caller, the other does not.
func leakOneReturnKeepsIt(b bool) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(context.Background())
	if b {
		return ctx, cancel
	}
	return ctx, nil
}

// The first of several uncovered returns is the one reported.
func leakFirstOfTwoReturns(b bool) error {
	ctx, cancel := context.WithCancel(context.Background())
	_ = ctx
	if b {
		return nil
	}
	if !b {
		return nil
	}
	cancel()
	return nil
}

// A bare block does not change the flow, and the return after it is reachable.
func leakInBlock(b bool) error {
	{
		ctx, cancel := context.WithCancel(context.Background())
		_ = ctx
		if b {
			cancel()
		}
	}
	return nil
}
