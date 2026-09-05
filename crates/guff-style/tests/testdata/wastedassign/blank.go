package wastedassign

// `_` denotes no location, so a store through it is not an assignment anyone
// could read — go/ssa's `addr` returns its `blank` lvalue and allocates
// nothing. guff's `Builder::address` was missing that arm, and the one caller
// that did not guard blank itself was the `select` comm clause: `case _, ok :=
// <-ch:` built a real local named `_` and stored into it, which wastedassign
// reported. celestia-node `blob/service_test.go` has two.
//
// Only a `select` with a second case reaches that path; a one-case `select`
// lowers to a plain receive.

func blankTwoCasesLoop(subCh, done chan int) {
	for {
		select {
		case _, ok := <-subCh:
			if !ok {
				return
			}
		case <-done:
			return
		}
	}
}

func blankTwoCasesNoLoop(subCh, done chan int) bool {
	select {
	case _, ok := <-subCh:
		return ok
	case <-done:
		return false
	}
}

func blankInClosure(subCh, done chan int) func() bool {
	return func() bool {
		select {
		case _, ok := <-subCh:
			return ok
		case <-done:
			return false
		}
	}
}

func blankOneCase(subCh chan int) bool {
	select {
	case _, ok := <-subCh:
		return ok
	}
}

func blankPlainAssign(subCh chan int) bool {
	_, ok := <-subCh
	return ok
}

func blankRange(m map[string]int) int {
	n := 0
	for _, v := range m {
		n += v
	}
	return n
}

func blankTypeAssert(x any) bool {
	_, ok := x.(int)
	return ok
}

// Control: a *named* receive variable in the same two-case `select` shape,
// overwritten before it is read. This must still be reported — otherwise the
// fix above would read as "wastedassign stopped looking inside `select`".
func namedInSelectIsStillReported(subCh, done chan int) int {
	v := 0
	select {
	case v = <-subCh:
	case <-done:
	}
	v = 7
	return v
}
