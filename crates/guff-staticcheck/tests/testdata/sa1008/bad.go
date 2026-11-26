package main

import "net/http"

const hostHeader = "host"

// A named non-canonical key: upstream wraps it in `http.CanonicalHeaderKey`
// rather than rewriting the constant.
func readNamed(h http.Header) []string {
	return h[hostHeader]
}

// An assignment with no `http.Header` index on its left is walked into, so the
// read on the right is reported. Assigning to a parameter keeps S1021 out of
// the fixture — its fix would overlap SA1008's on the same line, and
// golangci-lint's fixer then rewrites neither, which would quietly retire this
// file from the `--fix` tier.
func assignFromHeader(h http.Header, v []string) []string {
	v = h[hostHeader]
	return v
}

func main() {
	h := http.Header{}
	_ = h["foo"]
	var r http.Request
	_ = r.Header["etag"]
	_ = readNamed(h)
	_ = assignFromHeader(h, nil)
}
