package main

import "context"

func fn1(ctx context.Context) {}

func main() {
	fn1(context.TODO())
	fn1(context.Background())
}
