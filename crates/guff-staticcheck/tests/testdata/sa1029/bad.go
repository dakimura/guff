package main

import "context"

type T3 struct {
	A []int
}

type ctxKey struct{}
type ctxKeyField struct{ n int }
type namedInt int

func main() {
	var ctx context.Context
	var val any
	context.WithValue(ctx, "hi", val)
	context.WithValue(ctx, T3{}, val)
	var empty struct{}
	context.WithValue(ctx, struct{}{}, val)
	context.WithValue(ctx, empty, val)

	// A genuinely anonymous empty struct, however it is spelled.
	k6 := struct{}{}
	context.WithValue(ctx, k6, val)

	// Not empty, but still anonymous: comparable, so only the empty-struct
	// arm would fire, and it does not.
	k13 := struct{ n int }{n: 1}
	context.WithValue(ctx, k13, val)

	// Uncomparable.
	k14 := []int{1}
	context.WithValue(ctx, k14, val)

	// A named type built from an anonymous literal, and a named type
	// assigned one after the fact. `emit_store` converts each to the
	// declared type, so the value's type is `ctxKey` and SA1029 —
	// which tests `T.(*types.Struct)` with no `Underlying()` — says
	// nothing. guff peeled that conversion off in `ssa_value_type` and
	// reported both; fiber had one of each.
	var k2 ctxKey = struct{}{}
	context.WithValue(ctx, k2, val)
	var k5 ctxKey
	k5 = struct{}{}
	context.WithValue(ctx, k5, val)

	// The same named type, spelled the ordinary ways.
	var k1 ctxKey = ctxKey{}
	context.WithValue(ctx, k1, val)
	k3 := ctxKey{}
	context.WithValue(ctx, k3, val)
	var k4 ctxKey
	context.WithValue(ctx, k4, val)
	context.WithValue(ctx, ctxKey{}, val)
	context.WithValue(ctx, &ctxKey{}, val)

	// A named struct with a field, and a named built-in type: neither is a
	// `*types.Struct` with no fields, and neither is a `*types.Basic`.
	var k12 ctxKeyField = ctxKeyField{n: 1}
	context.WithValue(ctx, k12, val)
	var k11 namedInt = 3
	context.WithValue(ctx, k11, val)
}
