// Package source exists for the `source:` condition, which is the one
// condition matched against the file's text rather than against the issue.
package source

func mkerr() error { return nil }

// Run has two identical findings that differ only in the source line they sit
// on, so a `source:` rule can pick out one of them.
func Run() {
	mkerr() // exclude-this-line
	mkerr()
}
