package p

import "context"

func consume(ctx context.Context) { _ = ctx }

// No context parameter, and it manufactures one: every caller that has a
// context should have passed it down.
func helper() {
	consume(context.Background())
}

func Bad(ctx context.Context) {
	_ = ctx
	helper()
	consume(context.Background())
}

// A capturing func literal is a MakeClosure, which go/ssa gives no position;
// the report has to fall back to the literal's own `func` token.
func BadClosure(ctx context.Context) {
	n := 1
	consume(ctx)
	defer func() { _ = n; helper() }()
}
