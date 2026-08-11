//go:build custom

// This file is in the package only when `custom` is among the build tags.
// `run.build-tags` is handed to go/packages as `-tags`, so the file is not
// merely skipped by the linters — it is not loaded, and its declarations do not
// exist for the ones that do run.
package tags

import "fmt"

// RunTagged carries one errcheck finding and one govet/printf finding.
func RunTagged() {
	mkerr()
	fmt.Printf("%d\n", "not a number")
}
