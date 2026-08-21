package gosec_g118

// G118 (securego/gosec analyzers/context_propagation.go) is one analyzer id
// covering three unrelated checks, so this fixture is organised by check.
//
// The lost-cancel half is the interesting one: the rule is trivial ("was
// `cancel` ever called?") and everything in the port is the set of ways a
// cancel legitimately escapes without a visible call. Each case below is
// marked `// FINDING` or `// silent`, and compat/golden/cases/gosec runs
// golangci-lint 2.12.2 over this same file — so the marks are checked against
// upstream rather than asserted here.

import (
	"context"
	"net/http"
	"time"
)

// --- lost cancel: reported ---------------------------------------------------

func plainDrop(ctx context.Context) context.Context {
	c, cancel := context.WithCancel(ctx) // FINDING: nothing ever calls cancel
	if cancel == nil {
		return nil
	}
	return c
}

// `c, _ := …` is *reported*, which is the shape most likely to be read as a
// false positive: go/ssa emits the `Extract #1` before the blank lvalue throws
// it away, so the rule sees a cancel with no referrers at all.
func blankCancel(ctx context.Context) context.Context {
	c, _ := context.WithCancel(ctx) // FINDING
	return c
}

func timeoutDrop(ctx context.Context) {
	_, cancel := context.WithTimeout(ctx, time.Second) // FINDING
	_ = cancel != nil                                  // compared, never called
}

type mapHolder struct {
	cancels map[string]context.CancelFunc
}

// A cancel parked in a *map* is not tracked: `MapUpdate` is not in the walk's
// instruction set. dapr's subscriber.retrySubscription is this shape.
func (m *mapHolder) store(ctx context.Context, key string) {
	_, cancel := context.WithCancel(ctx) // FINDING
	m.cancels[key] = cancel
}

type deadField struct {
	cancel context.CancelFunc
}

// Stored in a struct field that nothing ever calls, and the struct is returned
// as a fresh pointer (an `Alloc`, not a load) so the "responsibility
// transferred" escape does not apply either.
func newDeadField(ctx context.Context) *deadField {
	_, cancel := context.WithCancel(ctx) // FINDING
	return &deadField{cancel: cancel}
}

// --- lost cancel: silent -----------------------------------------------------

func calledDirectly(ctx context.Context) {
	_, cancel := context.WithCancel(ctx) // silent
	cancel()
}

func calledByDefer(ctx context.Context) {
	_, cancel := context.WithTimeout(ctx, time.Second) // silent
	defer cancel()
}

func returnedToCaller(ctx context.Context) (context.Context, context.CancelFunc) {
	return context.WithCancel(ctx) // silent: responsibility is the caller's
}

func namedAndReturned(ctx context.Context) (context.Context, context.CancelFunc) {
	c, cancel := context.WithDeadline(ctx, time.Now()) // silent
	return c, cancel
}

func capturedByClosure(ctx context.Context) func() {
	_, cancel := context.WithCancel(ctx) // silent: the closure calls it
	return func() {
		cancel()
	}
}

func passedAsArgument(ctx context.Context) {
	_, cancel := context.WithCancel(ctx) // silent: used in a call
	register(cancel)
}

func register(f context.CancelFunc) {}

type liveField struct {
	cancel context.CancelFunc
}

func newLiveField(ctx context.Context) *liveField {
	_, cancel := context.WithCancel(ctx) // silent: Close calls the field
	return &liveField{cancel: cancel}
}

func (l *liveField) Close() {
	l.cancel()
}

var globalCancel context.CancelFunc

func startGlobal(ctx context.Context) {
	_, cancel := context.WithCancel(ctx) // silent: shutdown calls the global
	globalCancel = cancel
}

func shutdown() {
	globalCancel()
}

// --- generics: identical() does not see through an instantiation -------------

type genericHolder[T any] struct {
	cancel context.CancelFunc
}

// `genericHolder[T]` in this function is an *instance* whose type argument is
// this function's type parameter; `(*genericHolder[T]).Stop`'s receiver is the
// generic origin, with no type arguments. `types.Identical` says no, so the
// field walk misses the call and the cancel is reported — even though `Stop`
// plainly calls it. dapr's pluggable.GRPCConnector is this shape.
func newGenericHolder[T any](ctx context.Context) *genericHolder[T] {
	_, cancel := context.WithCancel(ctx) // FINDING
	return &genericHolder[T]{cancel: cancel}
}

func (g *genericHolder[T]) Stop() {
	g.cancel()
}

// --- goroutine on a detached context -----------------------------------------

func goDirect(ctx context.Context) {
	go work(context.Background()) // FINDING: ctx was available
}

func goViaClosure(r *http.Request) {
	go func() { // FINDING: the closure reaches context.Background
		work(context.Background())
	}()
	_ = r.Context()
}

func goWithRequestContext(r *http.Request) {
	ctx := r.Context()
	go work(ctx) // silent
}

// No request-scoped context in scope, so nothing was detached.
func goWithoutContextParam() {
	go work(context.Background()) // silent
}

func work(ctx context.Context) {}

// --- long-running loop without a Done guard ----------------------------------

// A strongly-connected region with no edge leaving it, a blocking call inside,
// and no `ctx.Done()`. The reported position is the blocking call, not the
// `for`.
func spin(ctx context.Context) {
	for {
		time.Sleep(time.Second) // FINDING
	}
}

func spinGuarded(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
		default:
		}
		time.Sleep(time.Second) // silent: the region has a Done guard
	}
}

// `return` gives the region an edge out of itself, so it is not a "loop
// region" as far as the rule is concerned — which is why almost no real loop
// reaches this check.
func spinWithExit(ctx context.Context) {
	for {
		time.Sleep(time.Second) // silent
		if ctx.Err() != nil {
			return
		}
	}
}

func spinNoBlocking(ctx context.Context) {
	n := 0
	for { // silent: nothing in the region blocks
		n++
		_ = n
	}
}
