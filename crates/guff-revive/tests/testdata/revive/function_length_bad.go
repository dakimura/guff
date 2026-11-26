// Package funclen has one over-long function and no empty-bodied one.
//
// function-length needs a file to itself: upstream's Apply returns as soon as
// it meets a function with an empty body, which silences the rest of the file
// (see crates/guff-revive/src/rules/function_length.rs). extended_bad.go has
// such a function near the top, so upstream reports nothing there.
package funclen

func tooManyStatements() {
	x := 0
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	x++
	_ = x
}
