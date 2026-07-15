package revivetest

import (
	"context"
	"errors"
	"fmt"
	"time"
)

import . "dot"

import _ "os"

var _ = X

var badError = errors.New("oops")

var wrongName = errors.New("bad")

var httpUrl = "x"

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

func badRange(m map[string]int) {
	for k, _ := range m {
		_ = k
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

func badErrorReturn() (error, int) {
	return nil, 0
}

func badContext(x int, ctx context.Context) {}

func badWithValue() {
	_ = context.WithValue(context.Background(), "key", 1)
}

func badVarDecl() {
	var n int = 0
	_ = n
}

var zeroVar int = 0

func badErrorsNew() error {
	return errors.New(fmt.Sprintf("x %d", 1))
}

type unexported struct{}

func ExportedUnexported() unexported {
	return unexported{}
}

func unusedParam(x int) {}

func unreachable() {
	return
	println("dead")
}

func indentElse() int {
	if true {
		return 1
	} else {
		return 2
	}
}

func superfluousElse() {
	if true {
		panic("x")
	} else {
		println("y")
	}
}
