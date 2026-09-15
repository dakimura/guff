// Package genericiface is about the conversion that records an interface.
//
// `typesImplementing` is built from the IR: every `MakeInterface` records the
// destination interface's method names against the named type being boxed, and
// a method on that list is skipped ("required to implement an interface").
//
// The conversion that produces the `MakeInterface` can happen at a call to a
// *generic* function — `newBookmark[parquet.Row](iter)` in grafana/tempo. guff
// read the callee's signature out of `Info.Types[e.Fun]`, which has no entry
// for the `IndexExpr` of an explicit instantiation, so no argument was
// converted, no `MakeInterface` was emitted, and `*rowIterator` stopped
// counting as an implementation: three `peekNextID - result 1 (error) is
// always nil` findings golangci-lint does not report.
package genericiface

type element interface{ ~int | ~string }

type cursor[T element] interface {
	next() (T, error)
	peek() (T, error)
}

// --- boxed at a call to a generic function: silent -------------------------

type viaGeneric struct{ n int }

func (i *viaGeneric) next() (int, error) { return 0, nil }

func (i *viaGeneric) peek() (int, error) {
	if i.n == 0 {
		return 0, nil
	}
	return i.n, nil
}

func takeGeneric[T element](x cursor[T]) {}

func runGeneric() {
	takeGeneric[int](&viaGeneric{})
}

// --- boxed at a call to a plain function: silent ---------------------------

type viaPlain struct{ n int }

func (i *viaPlain) next() (int, error) { return 0, nil }

func (i *viaPlain) peek() (int, error) {
	if i.n == 0 {
		return 0, nil
	}
	return i.n, nil
}

func takePlain(x cursor[int]) {}

func runPlain() {
	takePlain(&viaPlain{})
}

// --- never boxed at all: reported ------------------------------------------

type lone struct{ n int }

func (l *lone) solo() (int, error) { // FINDING: result 1 (error) is always nil
	if l.n == 0 {
		return 0, nil
	}
	return l.n, nil
}

func runLone() {
	l := &lone{}
	_, _ = l.solo()
}
