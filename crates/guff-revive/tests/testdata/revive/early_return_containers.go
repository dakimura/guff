// Package earlyreturncontainers is early-return over the same visitor: each
// chain ends in its container's own jump, so where the chain sits — last in a
// function, a loop, a case, a nested block — is what decides the message.
package earlyreturncontainers

func check(error) {}
func cond() bool  { return true }

func lastInFunc() {
	if cond() { // last statement of a function body
		check(nil)
	} else {
		return
	}
}

func notLast() {
	if cond() { // not last
		check(nil)
	} else {
		return
	}
	check(nil)
}

func lastInLoop(xs []int) {
	for range xs {
		if cond() { // last in a range body
			check(nil)
		} else {
			continue
		}
	}
}

func nestedLast() {
	if cond() {
		if cond() { // last inside an if body that is itself last
			check(nil)
		} else {
			return
		}
	}
}

func nestedNotLastParent() {
	if cond() {
		if cond() { // last inside an if body whose if is NOT last
			check(nil)
		} else {
			return
		}
	}
	check(nil)
}

func inCaseLast(n int) {
	switch n {
	case 1:
		if cond() { // last in a case clause
			check(nil)
		} else {
			break
		}
	}
}

func inLitLast() {
	f := func() {
		if cond() { // last in a func literal
			check(nil)
		} else {
			return
		}
	}
	f()
}

func elseIfChain() {
	if cond() { // if / else if / else
		check(nil)
	} else if cond() {
		check(nil)
	} else {
		return
	}
}

func inElseLast() {
	if cond() {
		check(nil)
	} else {
		if cond() { // last inside an else block
			check(nil)
		} else {
			return
		}
	}
}
