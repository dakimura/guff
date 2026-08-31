package goto_funclit_bad

// The other side of goto_funclit_ok.go: a func literal near a label must not
// *hide* a dead store either. gordonklaus/ineffassign reports all four of
// these; a fix that silences them is as wrong as one that reports the seven
// in goto_funclit_ok.go.

func mk() *int { return nil }

// F: func literal present, but no `goto` reaches back over the store.
func F() int {
	var p *int
	if p == nil {
		p = mk()
	}
	f := func() { _ = 1 }
	f()
	p = nil
	_ = p
	goto DONE
DONE:
	return 1
}

// G: the dead store is *inside* the func literal.
func G() func() {
	return func() {
		x := 1
		x = 2
		_ = x
	}
}

// I: back edge exists, but the store is overwritten before the label is reached.
func I() int {
	var p *int
RETRY:
	_ = p
	p = mk()
	p = nil
	_ = p
	if p == nil {
		goto RETRY
	}
	return 1
}

// J: a sibling function reusing the label name must not inherit I's edges.
func J(cond bool) int {
	var q *int
RETRY:
	_ = q
	q = mk()
	q = nil
	if cond {
		goto RETRY
	}
	return 2
}
