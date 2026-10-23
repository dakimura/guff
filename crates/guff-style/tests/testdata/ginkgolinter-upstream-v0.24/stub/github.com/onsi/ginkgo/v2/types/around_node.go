package types

import (
	"context"
)

type AroundNodeAllowedFuncs interface {
	~func(context.Context, func(context.Context)) | ~func(context.Context) context.Context | ~func()
}
type AroundNodeFunc func(ctx context.Context, body func(ctx context.Context))

func AroundNode[F AroundNodeAllowedFuncs](f F, cl CodeLocation) AroundNodeDecorator {
	panic("stub")
}

type AroundNodeDecorator struct {
	Body         AroundNodeFunc
	CodeLocation CodeLocation
}
type AroundNodes []AroundNodeDecorator

func (an AroundNodes) Clone() AroundNodes {
	panic("stub")
}
func (an AroundNodes) Append(other ...AroundNodeDecorator) AroundNodes {
	panic("stub")
}
