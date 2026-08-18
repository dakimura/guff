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
