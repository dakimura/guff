// Package sa1019structkey: staticcheck v0.8.1 (golangci-lint 2.14.0) reports a
// deprecated field named as a struct literal's key. guff's type checker did
// not record the key as a use of the field (go/types' `check.recordUse(key,
// fld)`), so the branch found no object and controller-runtime's
// `reconcile.Result{Requeue: true}` went unreported. The selector and the
// package-level initializer (Ginkgo's `var _ = Describe(...)`) are controls.
package sa1019structkey

import "example.com/sa1019structkey/dep"

func Literal() dep.Result { return dep.Result{Requeue: true} }

func Selector(r dep.Result) bool { return r.Requeue }

var _ = func() dep.Result { return dep.Result{Requeue: true} }()

func Live() dep.Result { return dep.Result{RequeueAfter: 1} }
