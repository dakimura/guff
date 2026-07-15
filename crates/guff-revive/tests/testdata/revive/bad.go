package revivetest

import (
	"errors"
	"fmt"
	"time"
)

import . "dot"

import _ "os"

var _ = X

var badError = errors.New("oops")

var wrongName = errors.New("bad")

type widget struct{}

func (self *widget) Alpha() {}

func (w *widget) Beta() {}

func increment() {
	i := 0
	i += 1
}

func emptyRange(m map[string]int) {
	for range m {
	}
}

func shadow() {
	len := 1
	_ = len
}

func durationName() time.Duration {
	var timeoutSecs time.Duration
	return timeoutSecs
}

func badErrorf() error {
	return fmt.Errorf("Bad error.")
}
