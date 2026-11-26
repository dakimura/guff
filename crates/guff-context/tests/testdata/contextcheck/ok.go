package contextcheck_ok

import "context"

func okWithValue(ctx context.Context) {
	_ = context.WithValue(ctx, "k", "v")
}

func okPassThrough(ctx context.Context) {
	consume(ctx)
}

func consume(ctx context.Context) {}
