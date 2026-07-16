//go:build go1.19

package atomictypes

import (
	"sync/atomic"
	myatomic "sync/atomic"
)

type X struct {
	x int32
}

type Z struct {
	y int64
	z int64
}

func goodLocal() {
	var x int32
	atomic.AddInt32(&x, 1)
	atomic.LoadInt32(&x)
}

func goodShadowAlias() {
	var x int32
	myatomic.AddInt32(&x, 1)
}

func goodField(wrapper *Z) {
	var y X
	atomic.CompareAndSwapInt32(&y.x, 2, 3)
	atomic.CompareAndSwapInt64(&wrapper.y, 2, 3)
}

func noInitAssign() {
	var x2 int32 = 5
	atomic.AddInt32(&x2, 1)
}

func unsyncLoad() {
	var z int32
	_ = z
	if z == 0 {
		atomic.LoadInt32(&z)
	}
}

type Y int32

func (y Y) dontFix(x int32) (result int32) {
	atomic.AddInt32(&x, 1)
	atomic.StoreInt32(&result, 100)
	atomic.AddInt32((*int32)(&y), 1)
	w := Z{
		z: 1,
	}
	atomic.AddInt64(&w.z, 1)
	return
}
