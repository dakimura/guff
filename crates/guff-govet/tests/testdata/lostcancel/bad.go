package p

import "context"

func f() {
	_, _ = context.WithCancel(context.Background())
}

func nested() {
	go func() {
		ctx, _ := context.WithCancel(context.Background())
		_ = ctx
	}()
}
