// Package ifelsecontainers places one if/return/else chain in every place
// revive's ifelse visitor can meet one (revive v1.15.0
// `internal/ifelse.visitor`). The trailing comment on each `if` says what
// golangci-lint 2.12.2 reports there. guff used to visit only function
// bodies and the `if`s directly in them, and missed ten of these thirteen.
package ifelsecontainers

func check(error) {}
func mk() error   { return nil }
func cond() bool  { return true }

func topLevel(bad bool) {
	if bad { // func decl body
		return
	} else {
		check(nil)
	}
	check(nil)
}

func inFuncLit(bad bool) {
	f := func() {
		if bad { // func literal in a decl
			return
		} else {
			check(nil)
		}
		check(nil)
	}
	f()
}

func inFor(bad bool) {
	for i := 0; i < 2; i++ {
		if bad { // for body
			return
		} else {
			check(nil)
		}
		check(nil)
	}
}

func inRange(xs []int) {
	for range xs {
		if cond() { // range body
			return
		} else {
			check(nil)
		}
		check(nil)
	}
}

func inCase(n int) {
	switch n {
	case 1:
		if cond() { // case clause
			return
		} else {
			check(nil)
		}
		check(nil)
	}
}

func inSelect(ch chan int) {
	select {
	case <-ch:
		if cond() { // select clause: not a chain upstream
			return
		} else {
			check(nil)
		}
		check(nil)
	}
}

func inBlock() {
	{
		if cond() { // plain block
			return
		} else {
			check(nil)
		}
		check(nil)
	}
}

func inCond() {
	if func() bool {
		if cond() { // func literal inside an if condition
			return true
		} else {
			check(nil)
		}
		return false
	}() {
		check(nil)
	}
}

func inGo() {
	go func() {
		if cond() { // go func literal
			return
		} else {
			check(nil)
		}
		check(nil)
	}()
}

func nestedIf() {
	if cond() {
		if cond() { // nested in an if body
			return
		} else {
			check(nil)
		}
		check(nil)
	}
	check(nil)
}

func superfluousLoop(xs []int) {
	for range xs {
		if cond() { // continue then else
			continue
		} else {
			check(nil)
		}
		check(nil)
	}
}

func superfluousCase(n int) {
	switch n {
	case 1:
		if cond() { // break then else
			break
		} else {
			check(nil)
		}
		check(nil)
	}
}

func labeled() {
L:
	for {
		if cond() { // labeled for
			break L
		} else {
			check(nil)
		}
	}
}

func inVarInit() {
	var f = func() {
		if cond() { // func literal in a var decl
			return
		} else {
			check(nil)
		}
		check(nil)
	}
	f()
}

var pkgLevel = func() {
	if cond() { // package-level func literal
		return
	} else {
		check(nil)
	}
	check(nil)
}
