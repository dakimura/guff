package p

import "context"

func Bad(ctx context.Context, xs []string) {
	for _, x := range xs {
		ctx = context.WithValue(ctx, "k", x)
		_ = ctx
	}
}
