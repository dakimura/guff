package fatcontext

import "context"

func ok() {
	ctx := context.Background()
	for i := 0; i < 10; i++ {
		ctx := context.WithValue(ctx, "key", i)
		_ = ctx
	}
}
