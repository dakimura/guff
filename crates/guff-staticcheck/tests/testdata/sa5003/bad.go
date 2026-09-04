package main

func f()         {}
func cond() bool { return true }

var ch = make(chan struct{})
var n int

// Every loop below has no condition and no way out, so each `defer` in it is
// a defer that will never run. The scan is `ast.Inspect` over the whole body,
// so a `defer` counts wherever it sits — including inside a `select`, a
// `switch` or a labelled statement, which a walker that only descends into the
// statement kinds it names never reaches.

func bare() {
	for {
		defer f()
	}
}

func deferInSelect() {
	for {
		select {
		case <-ch:
			defer f()
		}
	}
}

func deferInSelectDefault() {
	for {
		select {
		default:
			defer f()
		}
	}
}

func deferInSwitch() {
	for {
		switch n {
		case 1:
			defer f()
		}
	}
}

func deferInTypeSwitch(v any) {
	for {
		switch v.(type) {
		case int:
			defer f()
		}
	}
}

func deferInLabelled() {
	for {
	again:
		defer f()
		_ = 0
		goto again
	}
}

// A `return` inside a function literal returns from the literal, not from the
// enclosing function, so the loop is still infinite.
func returnInFuncLit() {
	for {
		defer f()
		go func() {
			return
		}()
	}
}

// `continue` is a branch statement, but it is not `break`.
func continueOnly() {
	for {
		defer f()
		continue
	}
}

// No condition still means infinite, whatever the post statement does.
func postOnly() {
	for i := 0; ; i++ {
		defer f()
		_ = i
	}
}

// Both loops enclose the defer, so the check reports once per loop. Upstream
// reports twice too; golangci-lint collapses the pair in its own pipeline.
func nested() {
	for {
		for {
			defer f()
		}
	}
}
