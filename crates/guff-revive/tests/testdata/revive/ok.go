// Package revivetest is a clean fixture for revive.
package revivetest

import (
	"context"
	"errors"
	"fmt"
	"time"
)

var errClean = errors.New("clean")

type widget struct{}

// Work does work.
func (w *widget) Work() {}

func increment() {
	i := 0
	i++
}

func goodRange(m map[string]int) {
	for k, v := range m {
		_ = k
		_ = v
	}
}

func durationName() time.Duration {
	var timeout time.Duration
	return timeout
}

func goodErrorf() error {
	return fmt.Errorf("clean error")
}

func goodErrorReturn() (int, error) {
	return 0, nil
}

func goodContext(ctx context.Context, x int) {
	_ = ctx
	_ = x
}

type key struct{}

func goodWithValue() {
	_ = context.WithValue(context.Background(), key{}, 1)
}

func goodVarDecl() {
	n := 0
	_ = n
}

func goodErrorsNew() error {
	return fmt.Errorf("x %d", 1)
}

// Exported is exported for tests.
type Exported struct{}

// GoodExported returns an exported value.
func GoodExported() Exported {
	return Exported{}
}

// String returns a builtin string — must not trip unexported-return.
func (e Exported) String() string {
	return "ok"
}

// Err returns the predeclared error interface — must not trip unexported-return.
func (e Exported) Err() error {
	return nil
}

func usedParam(x int) {
	_ = x
}

func reachable() {
	if true {
		return
	}
	println("ok")
}

func noElse() int {
	if true {
		return 1
	}
	return 2
}
