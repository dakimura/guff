// Package funlendoc has a package doc comment, which is the whole point of
// this fixture: the production typecheck keeps *that* comment and drops
// every other one, so `file.comments` is non-empty while carrying none of
// the comments inside a function body. A re-parse guard that asks only
// "is the list empty" therefore skips the re-parse and subtracts nothing.
//
// k6 `internal/log/cloud/cloud.go` is the shape this came from.
package funlendoc

// DocFileCommentHeavy has 36 body lines, 12 of which are comments.
// With ignore-comments it counts as 24.
func DocFileCommentHeavy() {
	// comment line 0 explaining the next statement
	x0 := 0
	_ = x0
	// comment line 1 explaining the next statement
	x1 := 0
	_ = x1
	// comment line 2 explaining the next statement
	x2 := 0
	_ = x2
	// comment line 3 explaining the next statement
	x3 := 0
	_ = x3
	// comment line 4 explaining the next statement
	x4 := 0
	_ = x4
	// comment line 5 explaining the next statement
	x5 := 0
	_ = x5
	// comment line 6 explaining the next statement
	x6 := 0
	_ = x6
	// comment line 7 explaining the next statement
	x7 := 0
	_ = x7
	// comment line 8 explaining the next statement
	x8 := 0
	_ = x8
	// comment line 9 explaining the next statement
	x9 := 0
	_ = x9
	// comment line 10 explaining the next statement
	x10 := 0
	_ = x10
	// comment line 11 explaining the next statement
	x11 := 0
	_ = x11
}
