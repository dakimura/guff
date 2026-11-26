// Package code is half of the `run.tests` fixture: the half that is linted
// either way. Its twin, code_test.go, is the half `run.tests: false` removes.
package code

import "os"

func mkerr() error { return nil }

// Run has one errcheck finding, so the `run.tests: false` golden is a
// subtraction from the default one rather than an empty file.
func Run(f *os.File) {
	mkerr()
}
