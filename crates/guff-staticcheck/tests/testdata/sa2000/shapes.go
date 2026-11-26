package main

// Where the `Add` has to be, and where the `go` may be.
//
// Upstream is one pattern and nothing else:
//
//	(GoStmt (CallExpr (FuncLit _ call@(CallExpr (Symbol "(*sync.WaitGroup).Add") _):_) _))
//
// `call@(…):_` is head:tail, so only the **first** statement of the literal's
// body can match — and a leading `BlockStmt` is matched as its own list, so a
// bare block still counts. `code.Matches` walks the whole file, so the `go`
// itself may be anywhere, including inside another function literal.
//
// guff scanned the whole body and walked statements by hand, knowing about
// neither `switch`/`select` cases nor function literals. It therefore reported
// a *correct* `Add` that merely sat inside some enclosing goroutine and missed
// genuinely misplaced ones: grafana/loki's `wire_http2_test.go:318`.

import "sync"

type holder struct{ wg *sync.WaitGroup }

func addFirst() {
	var wg sync.WaitGroup
	go func() { // FINDING: Add is the first statement
		wg.Add(1)
		defer wg.Done()
	}()
	wg.Wait()
}

func addSecond() {
	var wg sync.WaitGroup
	go func() { // silent: something else comes first
		defer wg.Done()
		wg.Add(1)
	}()
	wg.Wait()
}

func addInsideIf(c bool) {
	var wg sync.WaitGroup
	go func() { // silent: an `if` is not a block the pattern unwraps
		if c {
			wg.Add(1)
		}
		defer wg.Done()
	}()
	wg.Wait()
}

func addInLeadingBlock() {
	var wg sync.WaitGroup
	go func() { // FINDING: a leading block is matched as its own list
		{
			wg.Add(1)
		}
		defer wg.Done()
	}()
	wg.Wait()
}

func addInDoubleLeadingBlock() {
	var wg sync.WaitGroup
	go func() { // FINDING: and it unwraps all the way down
		{
			{
				wg.Add(1)
			}
		}
		defer wg.Done()
	}()
	wg.Wait()
}

func addInBlockButSecond() {
	var wg sync.WaitGroup
	go func() { // silent: second *inside* the leading block
		{
			defer wg.Done()
			wg.Add(1)
		}
	}()
	wg.Wait()
}

func stmtThenBlock() {
	var wg sync.WaitGroup
	go func() { // silent: a non-block statement comes first
		_ = 1
		{
			wg.Add(1)
		}
	}()
	wg.Wait()
}

func goInsideSwitch(c int) {
	var wg sync.WaitGroup
	switch c {
	case 1:
		go func() { // FINDING: the `go` may be inside a switch case
			wg.Add(1)
			defer wg.Done()
		}()
	}
	wg.Wait()
}

func goInsideSelect(ch chan int) {
	var wg sync.WaitGroup
	select {
	case <-ch:
		go func() { // FINDING: or a select case
			wg.Add(1)
			defer wg.Done()
		}()
	}
	wg.Wait()
}

func goInsideAnotherGoroutine() {
	var outer sync.WaitGroup
	outer.Add(1)
	go func() {
		defer outer.Done()

		var inner sync.WaitGroup
		inner.Add(1) // silent: correct, and not the first statement of anything
		go func() {
			defer inner.Done()
		}()
		inner.Wait()
	}()
	outer.Wait()
}

func badInsideAnotherGoroutine() {
	var outer sync.WaitGroup
	outer.Add(1)
	go func() {
		defer outer.Done()

		var inner sync.WaitGroup
		go func() { // FINDING: reached only by walking into the outer literal
			inner.Add(1)
			defer inner.Done()
		}()
		inner.Wait()
	}()
	outer.Wait()
}

func withArgs() {
	var wg sync.WaitGroup
	go func(n int) { // FINDING: parameters do not matter
		wg.Add(n)
	}(1)
	wg.Wait()
}

func (h holder) pointerField() {
	go func() { // FINDING: `h.wg` is a *sync.WaitGroup
		h.wg.Add(1)
		defer h.wg.Done()
	}()
	h.wg.Wait()
}

func namedFuncValue() {
	var wg sync.WaitGroup
	f := func() { wg.Add(1) }
	go f() // silent: the pattern needs a literal at the call site
	wg.Wait()
}
