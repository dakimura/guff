package contextcheck_closures

import "context"

func work(ctx context.Context) error {
	_ = ctx
	return nil
}

func helper() error { return work(context.Background()) }

// Each literal below captures `n`, so go/ssa emits a `MakeClosure` — and gives
// that instruction no position at all. Upstream falls back to the callee's own
// position, which is the literal's `func` token, so the report lands on the
// `func` keyword rather than on the `defer` / `go` / assignment.
func deferred(ctx context.Context) {
	n := 1
	_ = work(ctx)
	defer func() { _ = n; _ = helper() }()
}

func spawned(ctx context.Context) {
	n := 1
	_ = work(ctx)
	go func() { _ = n; _ = helper() }()
}

func immediate(ctx context.Context) {
	n := 1
	_ = work(ctx)
	func() { _ = n; _ = helper() }()
}

func assigned(ctx context.Context) {
	n := 1
	_ = work(ctx)
	fn := func() { _ = n; _ = helper() }
	fn()
}

// A literal that captures nothing is a bare function value, and the call that
// carries it has a position of its own — the `defer` keyword.
func nocapture(ctx context.Context) {
	_ = work(ctx)
	defer func() { _ = helper() }()
}

// No context to inherit in the enclosing function: nothing to report.
func noCtx() {
	n := 1
	defer func() { _ = n; _ = helper() }()
}
