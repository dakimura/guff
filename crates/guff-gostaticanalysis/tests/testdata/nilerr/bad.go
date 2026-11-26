package nilerr

func do() error { return nil }

func badNotNil() error {
	err := do()
	if err != nil {
		return nil
	}
	return nil
}

func badIsNil() error {
	err := do()
	if err == nil {
		return err
	}
	return err
}

// `err.Error()` alone is *not* a use: upstream reads `callInstr.Call.Args`, and
// an invoke call keeps its receiver in `Call.Value`. Counting the receiver
// silenced this finding.
func badErrErrorCallOnly() error {
	err := do()
	if err != nil {
		msg := err.Error()
		_ = msg
		return nil
	}
	return nil
}

// The error is copied to a local and never passed anywhere.
func badErrAssignedOnly() error {
	err := do()
	if err != nil {
		kept := err
		_ = kept
		return nil
	}
	return nil
}

// The error result is there and is returned nil, so the position check must not
// widen into a way out of this one.
func badNilInErrorPosition() (*int, error) {
	err := do()
	if err != nil {
		return nil, nil
	}
	n := 1
	return &n, nil
}

// The hint after "error is not nil" is `fmt.Sprintf` over the lines the error
// value came from, and both of these shapes were rendered wrongly.

// NOT here, deliberately: a closure capturing `err`. Upstream names the line
// because go/ssa's `(*address).load` copies the address's position onto the
// load (`load.pos = a.pos`) and guff's `Address::load` drops it, so guff prints
// "lines []" there. Giving loads their position is the right fix and is
// measured — but it also un-suppresses two SA5011 findings in grafana that
// `sa5011.rs`'s `if pos == 0 { continue }` had been hiding, so it waits on
// that check. Adding the shape here now would record a gap this change cannot
// close.

// A value merged by a phi yields several lines, and Go renders a []int with
// `%v` — space-separated, `lines [N M]`. Rust's `{:?}` writes commas.
func MergedFromTwoBranches(c bool) error {
	var err error
	if c {
		err = errNotFound()
	} else {
		err = errOther()
	}
	if err != nil {
		return nil
	}
	return err
}

func errNotFound() error { return nil }
func errOther() error    { return nil }
