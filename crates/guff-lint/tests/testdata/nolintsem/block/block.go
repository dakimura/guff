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
