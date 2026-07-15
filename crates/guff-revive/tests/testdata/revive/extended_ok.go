// Package extendedtest is a clean fixture for revive extended rules.
package extendedtest

import (
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"time"
)

const (
	zero       = 0
	one        = 1
	year2023   = 2023
	monthJan   = 1
	day15      = 15
	hour12     = 12
	minute30   = 30
	second45   = 45
	nanosecond = 0
)

var counter int64

func goodAtomic() {
	atomic.AddInt64(&counter, one)
}

func goodReturn() int {
	return one
}

func goodBoolLiteral(x bool) bool {
	return x
}

func goodWaitGroup(wg *sync.WaitGroup) {
	wg.Add(one)
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
	if n <= zero {
		return zero
	}
	return goodRecursion(n - one)
}

func goodIfReturn() error {
	return errors.New("x")
}

func goodFormat() {
	_, _ = fmt.Print("hello")
}

func goodImportShadow() string {
	return fmt.Sprintf("%s", "ok")
}

func goodConstLogical(a int) bool {
	return a > zero
}

func goodTimeDate() time.Time {
	return time.Date(year2023, monthJan, day15, hour12, minute30, second45, nanosecond, time.UTC)
}

func goodUnhandledError() error {
	return errors.New("x")
}

type goodStructTag struct {
	Name string `json:"name,omitempty"`
}

func goodUnnecessaryStmt() int {
	return one
}

func goodArgLimit(a, b, c int) int {
	return a + b + c
}

func goodAddConstant() string {
	return "once"
}

func goodEarlyReturn(x int) {
	if x <= zero {
		return
	}
	_, _ = fmt.Print("big")
}

func goodDeepExit() {
	_, _ = fmt.Print("ok")
}

func getValue() int {
	return one
}

func goodUnnecessaryIf(flag bool) bool {
	return flag
}

func goodDefer() {
	defer func() {
		_ = recover()
	}()
}
