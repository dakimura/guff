package main

import "context"

type T3 struct {
	A []int
}

func main() {
	var ctx context.Context
	var val any
	context.WithValue(ctx, "hi", val)
	context.WithValue(ctx, T3{}, val)
	var empty struct{}
	context.WithValue(ctx, struct{}{}, val)
	context.WithValue(ctx, empty, val)
}
