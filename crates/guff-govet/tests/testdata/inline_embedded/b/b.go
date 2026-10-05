package b

import "example.com/inline/a"

// A qualified embedding names its field after the selector: still a rename.
type Q struct {
	a.Value
}

// An alias spelled with the same name as its target does not rename the
// field it declares, so this one is inlined.
//
//go:fix inline
type Val = a.Val

type R struct {
	Val
}

var _ a.Value
