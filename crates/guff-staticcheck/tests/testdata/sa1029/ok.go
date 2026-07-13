package main

import "context"

type T string

func main() {
	var ctx context.Context
	var key T
	var val any
	context.WithValue(ctx, key, val)
}
