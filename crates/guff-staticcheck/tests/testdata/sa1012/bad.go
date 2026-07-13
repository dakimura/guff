package main

import "context"

func fn1(ctx context.Context) {}

func fn2(x string, ctx context.Context) {}

type T struct{}

func (*T) Foo() {}

func main() {
	fn1(nil)
	fn1(context.TODO())
	fn2("", nil)

	_ = (func(context.Context))(nil)
	(*T).Foo(nil)
}
