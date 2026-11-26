// go:build linux

package spaced

// Upstream's `comment` only dispatches to goBuildLine when the text contains
// the exact substring "//go:build", so a space after // never reaches the
// "malformed //go:build line" report. Nothing is expected here.

func f() {}
