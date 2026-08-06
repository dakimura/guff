package p

import "context"

func Bad(ctx context.Context, xs []string) {
	for _, x := range xs {
		ctx = context.WithValue(ctx, "k", x)
		_ = ctx
	}
}

type holder struct {
	ctx    context.Context
	cancel context.CancelFunc
}

// Assigning struct fields in a plain method is not a nested context.
func (h *holder) Start() {
	h.ctx, h.cancel = context.WithCancel(context.Background())
}

// check-struct-pointers defaults to false.
func InFuncLit(h *holder) func() {
	return func() {
		h.ctx = context.WithValue(h.ctx, "k", "v")
	}
}
