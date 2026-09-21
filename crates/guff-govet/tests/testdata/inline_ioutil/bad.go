package inline_ioutil

import (
	"io"
	"io/ioutil"
	iou "io/ioutil"
	. "io/ioutil"
	"strings"
)

// Expression position, all six wrappers that carry `//go:fix inline`.
func expressions(p string, b []byte, r io.Reader, d, pat string) {
	_, _ = ioutil.TempDir(d, pat)
	_, _ = ioutil.ReadFile(p)
	_ = ioutil.WriteFile(p, b, 0o600)
	_, _ = ioutil.ReadAll(r)
	_ = ioutil.NopCloser(r)
	_, _ = ioutil.TempFile(d, pat)
}

// Statement position, results discarded. The version arm reports these too:
// `inline.Inline` compares versions before it looks at the call's context.
func statement(p string, b []byte) {
	ioutil.WriteFile(p, b, 0o600)
}

func deferred(p string, b []byte) {
	defer ioutil.WriteFile(p, b, 0o600)
}

func goStmt(p string, b []byte) {
	go ioutil.WriteFile(p, b, 0o600)
}

// Nested in another call's arguments.
func nested(s string) io.ReadCloser {
	return ioutil.NopCloser(strings.NewReader(s))
}

// Parenthesized callee: `call.Pos()` is the `(`, not the selector.
func parenCallee(p string) ([]byte, error) {
	return (ioutil.ReadFile)(p)
}

func closure(p string) func() ([]byte, error) {
	return func() ([]byte, error) { return ioutil.ReadFile(p) }
}

func compositeLit(r io.Reader) []io.ReadCloser {
	return []io.ReadCloser{ioutil.NopCloser(r)}
}

// Aliased import. The name upstream prints comes from the callee's package
// (`fn.Pkg().Name()`), so this reports as `ioutil.ReadFile`, not `iou.ReadFile`.
func aliased(p string) ([]byte, error) {
	return iou.ReadFile(p)
}

// Dot import: no selector at all, same callee, still reported.
func dotImported(p string) ([]byte, error) {
	return ReadFile(p)
}

// The caller shadows the forwarded-to package's name. (This whole file never
// imports `os`, so every shape above is also a caller that does not import it.)
func shadowsOs(p string) ([]byte, error) {
	os := 1
	_ = os
	return ioutil.ReadFile(p)
}

// Silent: a use that is not a call of the wrapper. `ReadDir` carries no
// directive, `ioutil.ReadFile` here is a value, `ioutil.Discard` is a var.
func notCalls(dirname string) (func(string) ([]byte, error), io.Writer) {
	_, _ = ioutil.ReadDir(dirname)
	return ioutil.ReadFile, ioutil.Discard
}

// Silent: called through a variable, so the callee is not the wrapper.
func throughVariable(p string) ([]byte, error) {
	f := ioutil.ReadFile
	return f(p)
}
