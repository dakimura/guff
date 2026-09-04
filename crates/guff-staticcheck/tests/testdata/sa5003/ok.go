package main

func f()         {}
func cond() bool { return true }

var ch = make(chan struct{})
var n int

// A loop that can be left is not an infinite loop, and the scan finds the way
// out wherever it is written.

func finite() {
	for cond() {
		defer f()
	}
}

func plainBreak() {
	for {
		defer f()
		break
	}
}

func breakInIf() {
	for {
		defer f()
		if cond() {
			break
		}
	}
}

// kubeshark `cmd/console.go`: the way out is a `return` inside a `select`.
func returnInSelect() {
	for {
		defer f()
		select {
		case <-ch:
			return
		}
	}
}

func returnInSelectIf() {
	for {
		defer f()
		select {
		case <-ch:
			if cond() {
				return
			}
		}
	}
}

func returnInSwitch() {
	for {
		defer f()
		switch n {
		case 1:
			return
		}
	}
}

// A `break` inside a `switch` or a `select` leaves that statement, not the
// loop. Upstream's own TODO calls this a false negative and counts it as a way
// out anyway; matching it is what keeps the two tools equal.
func breakInSwitch() {
	for {
		defer f()
		switch n {
		case 1:
			break
		}
	}
}

func breakInSelect() {
	for {
		defer f()
		select {
		case <-ch:
			break
		}
	}
}

// Same rule one loop in: the `break` leaves the inner loop, and it still
// counts.
func breakInInnerLoop() {
	for {
		defer f()
		for {
			break
		}
	}
}

func labelledBreak() {
outer:
	for {
		defer f()
		select {
		case <-ch:
			break outer
		}
	}
}

// The defer belongs to the function literal, which returns on every call.
func deferInFuncLit() {
	for {
		func() {
			defer f()
		}()
	}
}
