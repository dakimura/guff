package main

import "example.com/old"

func main() {
	old.Legacy()
	var _ old.OldClient

	// A field selection is recorded in `Info.Selections` exactly like a method
	// one, so "is there a selection" answers the wrong question: this took the
	// method branch, looked for `Options.Old` among the methods, and missed.
	var o old.Options
	o.Old = 1
	_ = o.Fine

	// Promoted through an embedded value, an embedded pointer, and a named
	// field — the same deprecated `Options.Old` reached three more ways.
	var w old.Wrapper
	w.Old = 2
	_ = w.Old

	var pw old.PtrWrapper
	pw.Old = 3

	var h old.Holder
	h.Cfg.Old = 4

	// Writing the embedding out by hand selects the same field.
	w.Options.Old = 5
}
