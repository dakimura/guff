// Package vardeclotherpkg covers `var-declaration`'s cross-package gate.
//
// Upstream drops the finding when the right-hand side reaches into another
// package — revive type-checks with its own `lint.Package`, so such an operand
// comes back invalid and the rule bails before it reports. guff used to ask a
// narrower question, "is any identifier an import *name*", which only ever sees
// the qualifier in `pkg.X`. Every row below without a qualifier was reported by
// guff and by nobody else.
//
// The dot-import rows are velero's `test/e2e`, which writes
// `var NodePortTest func() = TestFunc(&NodePort{})` under a dot
// import 28 times.
package vardeclotherpkg

import (
	. "example.com/revive/vardeclother"

	qual "example.com/revive/vardeclother"
)

var localBox = Box{S: "x"}

func localFunc() string { return "" }

// Reaches into another package with no qualifier to notice: all silent.
var (
	dotCall   func() = TestFunc(&Case{Name: "x"})
	dotConst  int    = Answer
	dotType   Case   = Case{Name: "n"}
	pkgMethod string = localBox.Method()
	pkgField  string = localBox.S
)

// The same reach, with a qualifier: silent for the same reason.
var (
	qualCall  string = qual.Str()
	qualConst int    = qual.Answer
)

// Declared here, so the rule reports. This is the row that keeps the gate from
// silencing everything.
var localCall string = localFunc()
