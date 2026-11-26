package main

import "time"

// The pattern is `(CommClause (UnaryExpr "<-" (CallExpr (Symbol "time.After")
// [arg])) body)`: a *bare* receive, and the select must have exactly one
// clause. The body may be empty or not — upstream only uses that to decide
// whether it can offer a fix.
func f() {
	select {
	case <-time.After(time.Second):
	}
}

func g() {
	select {
	case <-time.After(time.Second):
		println("woke up")
	}
}
