package reassign

import "context"

func consume(ctx context.Context) { _ = ctx }

// helper manufactures a context and hands it on, so every caller that has one
// of its own should have passed it down.
func helper() {
	consume(context.Background())
}

// NoReassign keeps its context, so the chain report is the only finding.
func NoReassign(ctx context.Context) {
	consume(ctx)
	helper()
}

// ReassignPlain replaces the context unconditionally. The store keeps the
// position of the `=` token, and collectCtxRef gives up on the function: the
// helper chain below is *not* reported.
func ReassignPlain(ctx context.Context) {
	ctx = context.Background()
	consume(ctx)
	helper()
}

// ReassignPhi replaces it on one branch only. Lifting turns that into a phi,
// which has no position of its own and inherits the parameter's.
func ReassignPhi(ctx context.Context, b bool) {
	if b {
		ctx = context.Background()
	}
	consume(ctx)
	helper()
}

// ReassignCaptured is the same, except a closure captures ctx so the cell is
// never lifted and the store reports where it stands.
func ReassignCaptured(ctx context.Context, b bool) {
	if b {
		ctx = context.Background()
	}
	f := func() { consume(ctx) }
	f()
	helper()
}

// ReassignLoop replaces it inside a loop.
func ReassignLoop(ctx context.Context, n int) {
	for i := 0; i < n; i++ {
		ctx = context.Background()
	}
	consume(ctx)
	helper()
}

type key struct{ k int }

// ReassignInherited derives the new context from the old one, which is what
// the message asks for: nothing is reported here, so the chain still is.
func ReassignInherited(ctx context.Context) {
	ctx = context.WithValue(ctx, key{}, 1)
	consume(ctx)
	helper()
}
