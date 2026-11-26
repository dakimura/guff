package p_test

import (
	"example.com/xt/p"
	"example.com/xt/q"
)

// Reveal comes from export_test.go. Describe proves the p.T named by q's
// signature is the same p.T this package builds: resolving the import to a
// second, separately checked copy of p would fail here even if Reveal resolved.
func check() int {
	v := p.New(6)
	return p.Reveal(v) + q.Describe(v)
}
