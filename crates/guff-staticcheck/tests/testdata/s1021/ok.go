package main

func f() {
	var x uint = 1
	_ = x
}

// Upstream's hasMultipleAssignments is a full ast.Inspect over the block, so a
// second assignment anywhere below — including inside a select, which a
// statement-kind-by-statement-kind walker misses — suppresses the merge.
// Shape from prometheus `discovery/kubernetes.retryOnError`.
func retryOnError(done <-chan struct{}, g func() error) bool {
	var err error
	err = g()
	for {
		if err == nil {
			return false
		}
		select {
		case <-done:
			return true
		default:
			err = g()
		}
	}
}

func reassignedInFuncLit(g func() error) func() {
	var err error
	err = g()
	_ = err
	return func() {
		err = g()
	}
}

func reassignedInLabeledLoop(g func() int) int {
	var n int
	n = g()
loop:
	for range 3 {
		n = g()
		if n == 0 {
			break loop
		}
	}
	return n
}
