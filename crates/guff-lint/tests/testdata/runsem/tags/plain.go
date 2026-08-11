// Package tags is half of the `run.build-tags` fixture: the half that has no
// build constraint and is therefore linted whatever the tags are. Its twin,
// tagged.go, sits behind `//go:build custom`.
package tags

func mkerr() error { return nil }

// Run has one errcheck finding, so the golden without the tag is a subtraction
// from the one with it rather than an empty file.
func Run() {
	mkerr()
}
