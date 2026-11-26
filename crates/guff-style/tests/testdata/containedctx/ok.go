package containedctxok

import "context"

type Ok struct {
	Name string
	N    int
}

type HoldsCancel struct {
	Cancel context.CancelFunc
}

func use(ctx context.Context) context.Context {
	return ctx
}
