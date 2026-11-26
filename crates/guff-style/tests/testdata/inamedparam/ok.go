package example

import "context"

// AllParamsNamed has names on every interface method parameter.
type AllParamsNamed interface {
	SingleParam(ctx context.Context) error
	Multi(ctx context.Context, n int) error
	NoParams()
}
