// Package tool repeats cmd's findings under a nested directory, so that a
// `path:` / `paths:` / `path-except:` pattern has somewhere to bite and
// somewhere to leave alone.
package tool

import "fmt"

func mkerr() error { return nil }

// Run mirrors cmd.Run's findings.
func Run() {
	mkerr()
	fmt.Println("no error check here")
	fmt.Printf("%d\n", "not a number")
}
