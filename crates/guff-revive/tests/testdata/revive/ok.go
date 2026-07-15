package revivetest

import (
	"errors"
	"fmt"
	"time"
)

// Package revivetest is a clean fixture for revive.
var errClean = errors.New("clean")

type widget struct{}

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
