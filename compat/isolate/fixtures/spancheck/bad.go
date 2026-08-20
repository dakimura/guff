package p

import (
	"context"

	"go.opentelemetry.io/otel"
)

func Bad(ctx context.Context) {
	_, span := otel.Tracer("app").Start(ctx, "op")
	_ = span
}

// Upstream reports the first message on the *assignment statement* and the
// second on the return its CFG search reaches — not on the closing brace, and
// not on the call's own column.
func StraightLineReturn(ctx context.Context) context.Context {
	c, span := otel.Tracer("app").Start(ctx, "op")
	_ = span
	return c
}

// A branch between the span and the return: upstream's search finds no return
// block it will name, and reports **nothing** — both messages come from one
// `if ret != nil`.
func BranchBeforeReturn(ctx context.Context, flag bool) context.Context {
	c, span := otel.Tracer("app").Start(ctx, "op")
	_ = span
	if flag {
		return c
	}
	return c
}

// A `for` counts as that branch too.
func LoopBeforeReturn(ctx context.Context, n int) context.Context {
	c, span := otel.Tracer("app").Start(ctx, "op")
	_ = span
	for i := 0; i < n; i++ {
		println(i)
	}
	return c
}

// The span and the return inside the same `if` body: that block ends in a
// return, so it is the one named.
func SpanInsideIf(ctx context.Context, flag bool) context.Context {
	if flag {
		c, span := otel.Tracer("app").Start(ctx, "op")
		_ = span
		return c
	}
	return ctx
}
