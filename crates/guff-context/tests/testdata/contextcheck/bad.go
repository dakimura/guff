package contextcheck

import "context"

func consume(c context.Context) {}

func inner() {
	consume(context.Background())
}

func badCall(ctx context.Context) {
	inner()
}

func badDirect(ctx context.Context) {
	consume(context.Background())
}

func badAssign(ctx context.Context) {
	ctx2 := context.Background()
	_ = ctx2
}
