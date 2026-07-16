package example

import "context"

type Another interface {
	Get() string
}

type NamedParam interface {
	Void()
	NoArgs() string

	SingleParam(context.Context) error

	WithName(ctx context.Context, number int, toggle bool) (bool, error)

	WithoutName(
		context.Context,
		int,
		bool,
		struct{ b bool },
	)
}
