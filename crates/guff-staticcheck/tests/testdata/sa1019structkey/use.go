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

// Generic literal types (staticcheck v0.8.2): the key's selector has an
// *ast.IndexExpr / *ast.IndexListExpr as its X. v0.8.1 panicked here.
func GenericIndex() dep.S2[int] { return dep.S2[int]{Field2: "x"} }

func GenericIndexList() dep.S3[int, bool] { return dep.S3[int, bool]{F: "x"} }

func GenericParamField() dep.S2[int] { return dep.S2[int]{GenField: 1} }

func GenericLive() dep.S2[int] { return dep.S2[int]{Live2: 1} }

func GenericMixed() dep.S3[string, int] { return dep.S3[string, int]{G: 1, H: "y", F: "x"} }

func GenericSelector(x dep.S2[int]) (string, int) { return x.Field2, x.GenField }

func GenericAlias() S2Int { return S2Int{Field2: "x"} }

type S2Int = dep.S2[int]

type local[T any] struct {
	// Deprecated: Use b instead.
	a string
	b T
}

func GenericLocal() local[int] { return local[int]{a: "x", b: 2} }

func GenericComposite() dep.S2[int] {
	return dep.S2[int]{Slice: nil, Hook: nil, GenSlice: nil}
}

func GenericCompositeSelector(x dep.S2[bool]) int { return len(x.Slice) + len(x.GenSlice) }
