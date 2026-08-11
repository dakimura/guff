// `run.tests` (default true) decides whether the loader asks go/packages for
// test packages at all. Everything in this file is invisible when it is false —
// including the findings, which is the point.
package code

import (
	"fmt"
	"testing"
)

// TestRun carries one errcheck finding and one govet/printf finding, so the
// setting's effect is visible for two different linters rather than one.
func TestRun(t *testing.T) {
	mkerr()
	fmt.Printf("%d\n", "not a number")
}
