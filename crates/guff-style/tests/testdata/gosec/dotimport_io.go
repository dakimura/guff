// Package dotimportio is G109 (strconv.Atoi feeding a narrowing conversion) and
// G110 (io.Copy from a decompressing reader) through dot imports.
//
// Both rules keep a side map keyed off a call list, and both lists are
// `ContainsPkgCallExpr` upstream: the tracking never starts for a dot-imported
// `Atoi`, and the `Copy` that would read the map is not recognised either.
// `gzip` stays qualified — its `Reader`/`Writer` would collide with `io`'s, and
// `net` cannot join this file at all (`net.Pipe` vs `io.Pipe`), which is why
// G102's dot-import shape lives in dotimport_net.go.
package dotimportio

import (
	"compress/gzip"
	"io"
	. "io"
	"strconv"
	. "strconv"
)

// silent for G109 — the dot-imported `Atoi` never enters the tracked set.
// (G115, an SSA analyzer that reads the callee's declaring package, still fires
// on the conversion: that is upstream's answer here too.)
func DotAtoi(s string) int32 {
	n, _ := Atoi(s)

	return int32(n)
}

// fires for G109 — package-qualified `strconv.Atoi`, same conversion.
func QualifiedAtoi(s string) int32 {
	n, _ := strconv.Atoi(s)

	return int32(n)
}

// silent for G110 — `gzip.NewReader` is tracked, but the dot-imported `Copy`
// is not on the copy list.
func DotCopy(r Reader, w Writer) {
	zr, _ := gzip.NewReader(r)
	_, _ = Copy(w, zr)
}

// fires for G110 — the same pair with `io.Copy` written out.
func QualifiedCopy(r io.Reader, w io.Writer) {
	zr, _ := gzip.NewReader(r)
	_, _ = io.Copy(w, zr)
}
