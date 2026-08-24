package p

import "context"

// fatcontext has four categories and the fixture used to reach one. Each is a
// separate message, so each is a separate position and a separate arm.

// categoryInLoop
func InLoop(ctx context.Context, xs []string) {
	for _, x := range xs {
		ctx = context.WithValue(ctx, "k", x)
		_ = ctx
	}
}

// categoryInFuncLit
func InFuncLit(ctx context.Context) func() {
	return func() {
		ctx = context.WithValue(ctx, "k", "v")
		_ = ctx
	}
}

type holder struct {
	ctx    context.Context
	cancel context.CancelFunc
}

// categoryInStructPointer — off by default (`DetectInStructPointers`), so it is
// the one arm this fixture states in settings.yml rather than assumes.
//
// `getCategory` tests the *enclosing node* first, so a loop around this would
// make it `categoryInLoop` instead: the pointer check only gets a turn when the
// assignment is not inside a `for`/`range`.
func InStructPointer(h *holder) {
	h.ctx = context.WithValue(h.ctx, "k", 1)
}
