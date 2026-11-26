package bad

import "sync/atomic"

func bad() {
	var x int64
	x = atomic.AddInt64(&x, 1)
}
