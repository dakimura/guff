package goto_funclit_ok

// A func literal is a nested function, but its labels are the only ones it may
// name: a `goto` never crosses the boundary. Walking one must therefore hide
// the enclosing function's labels without destroying them — the `goto`s below
// all still reach their label, so none of these stores is ineffectual.
//
// Measured against gordonklaus/ineffassign on all seven shapes; upstream keys
// its goto table on the label's `*ast.Object`, so it never resets it at all.
// nats-server `processSnapshot` (jetstream_cluster.go:10730) is shape `A`.

func mk() *int { return nil }

// A: func literal between the label and the `goto`, which sits in a select.
func A(ch chan int) int {
	var p *int
RETRY:
	if p == nil {
		p = mk()
		if p == nil {
			return 0
		}
	}
	f := func() { _ = 1 }
	f()
	p = nil
	for {
		select {
		case <-ch:
			goto RETRY
		}
	}
}

// B: the same without the func literal — the shape that already worked.
func B(ch chan int) int {
	var p *int
RETRY:
	if p == nil {
		p = mk()
		if p == nil {
			return 0
		}
	}
	p = nil
	for {
		select {
		case <-ch:
			goto RETRY
		}
	}
}

// C: func literal *before* the label, so the label records its block after.
func C(ch chan int) int {
	var p *int
	f := func() { _ = 1 }
	f()
RETRY:
	if p == nil {
		p = mk()
		if p == nil {
			return 0
		}
	}
	p = nil
	for {
		select {
		case <-ch:
			goto RETRY
		}
	}
}

// D: func literal between; the `goto` is in a plain if.
func D(cond bool) int {
	var p *int
RETRY:
	if p == nil {
		p = mk()
		if p == nil {
			return 0
		}
	}
	f := func() { _ = 1 }
	f()
	p = nil
	if cond {
		goto RETRY
	}
	return 1
}

// E: func literal between; the `goto` is in a plain for.
func E(cond bool) int {
	var p *int
RETRY:
	if p == nil {
		p = mk()
		if p == nil {
			return 0
		}
	}
	f := func() { _ = 1 }
	f()
	p = nil
	for {
		if cond {
			goto RETRY
		}
		return 1
	}
}

// H: the func literal carries a label of its own with the *same name*. The
// inner `goto RETRY` must bind to the inner label and leave the outer alone.
func H(ch chan int) int {
	var p *int
RETRY:
	if p == nil {
		p = mk()
		if p == nil {
			return 0
		}
	}
	f := func() {
		i := 0
	RETRY:
		i++
		if i < 3 {
			goto RETRY
		}
	}
	f()
	p = nil
	for {
		select {
		case <-ch:
			goto RETRY
		}
	}
}

// M: a *forward* goto whose label sits below a func literal. The `goto` records
// its source before the literal is walked, so losing the table there drops the
// edge and `p = mk()` looks dead. (The literal has to be called in place — Go
// forbids a goto that jumps over a declaration.)
func M(cond bool) int {
	var p *int
	p = mk()
	if cond {
		goto SKIP
	}
	p = nil
	func() { _ = 1 }()
SKIP:
	if p == nil {
		return 0
	}
	return 1
}
