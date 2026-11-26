package fatcontext

import "context"

func ok() {
	ctx := context.Background()
	for i := 0; i < 10; i++ {
		ctx := context.WithValue(ctx, "key", i)
		_ = ctx
	}
}

type holder struct {
	ctx    context.Context
	cancel context.CancelFunc
}

// Upstream only inspects loops and function literals, so a plain method body
// is never a nested context.
func (h *holder) start() {
	h.ctx, h.cancel = context.WithCancel(context.Background())
}
