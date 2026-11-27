// Package b builds on its own; its in-package test imports a, which
// imports b, so `b [b.test]` is a cycle that only exists with tests on.
package b

// B is called by a.
func B() int { return 1 }
