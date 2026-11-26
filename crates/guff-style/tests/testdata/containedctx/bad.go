package containedctx

import "context"

type Bad struct {
	Ctx context.Context
}

type AlsoBad struct {
	Name string
	Ctx  context.Context
	N    int
}
