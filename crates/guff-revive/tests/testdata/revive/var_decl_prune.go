// Package vardeclprune covers what upstream's `var-declaration` visitor does
// *not* look at: every path of its `*ast.ValueSpec` case returns nil, so the
// walk never descends into a declaration's value. A ginkgo suite lives inside
// `var _ = Describe("…", func() { … })`, and upstream reports nothing in there.
package vardeclprune

// TopLevel is a plain declaration and is reported.
var TopLevel uint64 = 0

var fn = func() {
	var inClosure uint64 = 0
	_ = inClosure
}

var _ = func() int {
	var inBlank int = 0
	return inBlank
}
