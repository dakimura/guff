package p

import "context"

func helper() { // should take context
}

func Bad(ctx context.Context) {
	_ = ctx
	helper()
}
