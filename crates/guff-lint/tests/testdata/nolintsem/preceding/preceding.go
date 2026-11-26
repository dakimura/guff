// Package preceding covers the range expander: a directive on its own line
// suppresses the node that starts on the next line, but only when that node
// starts in the directive's own column (rangeExpander.Visit compares
// `nodeStartPos.Column == r.col`).
package preceding

func mkerr() error { return nil }

// Column matches: the directive is at column 1 and so is the FuncDecl, so the
// whole function body is covered.
//
//nolint:errcheck
func WholeFunc() {
	mkerr()
	mkerr()
}

func Indented() {
	// The directive is indented to the statement's column, so the statement is
	// covered.
	//nolint:errcheck
	mkerr()
}

func ColumnMismatch() {
//nolint:errcheck
	mkerr()
}

func BlankLineGap() {
	//nolint:errcheck

	mkerr()
}

// A GenDecl spanning several lines: the expansion has to reach the closing
// line of the declaration, not just the line after the directive.
//
//nolint:errcheck
var _ = func() int {
	mkerr()
	return 0
}()

//nolint:errcheck
func BlockBody() {
	if true {
		mkerr()
	}
}
