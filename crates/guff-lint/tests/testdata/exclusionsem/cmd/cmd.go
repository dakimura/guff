// Package cmd feeds the exclusion cases a spread of findings from four
// linters, so a rule can be written against each axis (linter, text, path,
// source) and the golden shows exactly which findings it removes.
package cmd

import (
	"fmt"
	"os"
)

func mkerr() error { return nil }

// Run produces, in order: an errcheck finding with no callee name (a call that
// is not a selector), one whose name matches the std-error-handling preset
// (`.*Close`), nothing at all for fmt.Println — errcheck's own default
// excluded symbols cover the fmt printers, which is why EXC0001 is not the
// thing that hides them — and a line carrying both a govet/printf finding and
// an errcheck one, which is what tells a `linters:` rule from a `text:` one.
func Run(f *os.File) {
	mkerr()
	f.Close()
	fmt.Println("no error check here")
	fmt.Printf("%d\n", "not a number")

	n := 1
	n = 2
	_ = n
}
