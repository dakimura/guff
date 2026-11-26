// Package block pins what a block comment does, which is nothing: the filter
// strips leading `/` and spaces (`strings.TrimLeft(text, "/ ")`), so `/*nolint`
// still starts with `*` and never matches `^nolint( |:|$)`.
package block

func mkerr() error { return nil }

func Same() {
	mkerr() /*nolint:errcheck*/
}

func Preceding() {
	/*nolint:errcheck*/
	mkerr()
}

func Nested() {
	/*
	   //nolint:errcheck
	*/
	mkerr()
}

// A directive above a `case` covers the clause's *body*, not just its first
// line: Go's CaseClause.End() is the end of the last body statement, and the
// range expander reads Node.End(). guff returned the colon for every clause,
// so the body went unsuppressed — a one-line node where Go has a block.
// (Found on grafana, 2026-08-13.)
func CaseClause(v int) error {
	switch {
	//nolint:errcheck
	case v == 1:
		mkerr()
		return nil
	default:
		return nil
	}
}
