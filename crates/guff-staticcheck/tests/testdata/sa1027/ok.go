package main

import "sync/atomic"

type T struct {
	A int64
	B int32
	C int64
}

func main() {
	var v T
	atomic.AddInt64(&v.A, 0)
	atomic.LoadInt64(&v.A)
}
