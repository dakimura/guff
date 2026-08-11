package main

import "time"

type clock struct{}

func (clock) After(d time.Duration) <-chan time.Time { return nil }

func f() { time.Sleep(time.Second) }

// `case t := <-time.After(d):` puts an AssignStmt in the Comm slot, and the
// pattern wants a bare `(UnaryExpr "<-" …)`. Upstream is silent; guff used to
// report it.
func assigned() {
	select {
	case t := <-time.After(time.Second):
		_ = t
	}
}

// `Symbol "time.After"` resolves the object, so another type's `After` method
// is not a match. guff used to fall back to the selector's name.
func otherAfter() {
	var c clock
	select {
	case <-c.After(time.Second):
	}
}

// More than one clause is not a sleep.
func twoClauses(ch chan int) {
	select {
	case <-time.After(time.Second):
	case <-ch:
	}
}

func withDefault() {
	select {
	case <-time.After(time.Second):
	default:
	}
}
