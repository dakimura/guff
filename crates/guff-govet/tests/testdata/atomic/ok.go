package ok

import "sync/atomic"

func ok() {
	var x int64
	atomic.AddInt64(&x, 1)
}
