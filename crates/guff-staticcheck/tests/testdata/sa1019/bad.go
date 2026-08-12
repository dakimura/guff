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
}
