// Package extendedtest is a clean fixture for revive extended rules.
package extendedtest

import (
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"time"
)

var counter int64

func goodAtomic() {
	atomic.AddInt64(&counter, 1)
}

func goodReturn() int {
	return 1
}

func goodBoolLiteral(x bool) bool {
	return x
}

func goodWaitGroup(wg *sync.WaitGroup) {
	wg.Add(1)
}

func goodStringOfInt(n int) string {
	_ = n
	return "42"
}

func goodTimeEqual(a, b time.Time) bool {
	return a.Equal(b)
}

func goodTypeAssert(v any) (int, bool) {
	x, ok := v.(int)
	return x, ok
}

func goodRecursion(n int) int {
	if n <= 0 {
		return 0
	}
	return goodRecursion(n - 1)
}

func goodIfReturn() error {
	return errors.New("x")
}

func goodFormat() {
	fmt.Print("hello")
}
