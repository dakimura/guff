package p

import (
	"fmt"
	"sync/atomic"
)

func BadAtomic() {
	var x int64
	x = atomic.AddInt64(&x, 1)
	_ = x
}

func BadPrintf() {
	fmt.Printf("%s", 1) // wrong type
}

func BadUnreachable() {
	return
	println("unreachable")
}
