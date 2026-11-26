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

// The same gate on the *left* operand. Upstream's line is
// `if !validType(lhsTyp) || !validType(rhsTyp)`, and guff only ever asked the
// right half, so `var x pkg.T = <local expr>` was reported by guff alone —
// ingress-nginx's `internal/ingress/controller/location.go:74` is
// `var el ingress.Location = *location`.

type ownCase qual.Case // defined *here*, underlying from another package

type aliasCase = qual.Case // an alias: a bare identifier for an imported type

var (
	localCase  Case
	localOwn   ownCase
	localAlias aliasCase
	casePtr    = &localCase
)

// Declared type reaches another package: silent, whatever the right-hand side.
var (
	lhsQualDeref qual.Case = *casePtr
	lhsQualVar   qual.Case = localCase
	lhsDotVar    Case      = localCase
)

// A local alias is a local *name*, so upstream reports it even though the type
// it denotes lives in another package. This row is the control that keeps the
// gate above from being written as "unalias and ask where the type lives",
// which would silence it.
var lhsAliasVar aliasCase = localAlias

// Declared here, so the rule reports — the underlying type living in another
// package makes no difference, because `ownCase` itself does not.
var lhsOwnVar ownCase = localOwn
