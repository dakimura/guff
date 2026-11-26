// Statements after a call that cannot return are not in the IR at all:
// `buildssa` hands go/ssa the `ctrlflow` no-return predicate, `emitCall` puts
// a `Panic` behind such a call and starts an unreachable block, and
// `deleteUnreachableBlocks` takes the rest away.
//
// nilness is the check that shows it: with the cut, the join block below has
// one live predecessor and `err` is provably nil there.
package nilness

import "log"

func g() (int, error) { return 0, nil }

// The abort does not return, so the second test is reached only on the path
// where err was nil.
func afterFatal() {
	v, err := g()
	if err != nil {
		log.Fatalf("boom: %v", err)
	}
	_ = v
	if err != nil { // impossible condition: nil != nil
		log.Println(err)
	}
}

// Same shape with a call that does return: the join block keeps both
// predecessors, err's nilness is unknown, and nothing is reported. Without
// this half, an implementation that cut after *every* call would still pass.
func afterReturningCall() {
	v, err := g()
	if err != nil {
		log.Println(err)
	}
	_ = v
	if err != nil {
		log.Println(err)
	}
}

// The cut is per call site, not per function: the code after the `if` is still
// reachable, because the abort is inside the branch.
func reachableAfterBranch(p *int) int {
	if p == nil {
		log.Fatalf("nil")
	}
	return *p
}
