package pkg

func fn() {
	var x, y int

	if x == 1 || x == 2 {
	} else if y == 3 {
	}

	if x == 1 || x == 2 {
	}

	for {
		if x == 1 || x == 2 {
		} else if x == 3 {
			break
		}
	}

	switch x {
	case 1, 2:
	case 3:
	}
}

type qfStatus struct{ n int }

type qfValues struct {
	Abandoned qfStatus
	Completed qfStatus
}

var qfStatusValues qfValues

// Upstream's `MayHaveSideEffects` answers true for a `*ast.UnaryExpr` whose
// operator is `&` or `<-`, whatever the operand is, so a comparison against an
// address disqualifies the chain. flipt compares
// `pr.Status == &azuregit.PullRequestStatusValues.Abandoned`.
func qfAddressOnTheRight(status *qfStatus) int {
	state := 0
	if status == &qfStatusValues.Abandoned {
		state = 1
	} else if status == &qfStatusValues.Completed {
		state = 2
	}
	return state
}

// The tag side counts too — the check is per operand, not per side.
func qfAddressOnTheLeft(status *qfStatus) int {
	state := 0
	if &qfStatusValues.Abandoned == status {
		state = 1
	} else if &qfStatusValues.Completed == status {
		state = 2
	}
	return state
}

// `<-` is the other operator that arm names. Two *different* channels: with
// one channel read twice the conditions are syntactically identical and SA4014
// takes over, which would make this function about a different check.
func qfChannelReceive(code int, a, b chan int) int {
	state := 0
	if code == <-a {
		state = 1
	} else if code == <-b {
		state = 2
	}
	return state
}
