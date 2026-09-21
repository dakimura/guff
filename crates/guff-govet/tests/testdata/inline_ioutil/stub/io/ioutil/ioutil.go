// Package ioutil stands in for GOROOT's `io/ioutil`: a closed set of
// deprecated wrappers, each a single forwarding call carrying `//go:fix
// inline`. The directive is not in export data, which is why guff carries the
// names in a table.
package ioutil

import (
	"io"
	"os"
)

//go:fix inline
func TempDir(dir, prefix string) (string, error) {
	return os.MkdirTemp(dir, prefix)
}

//go:fix inline
func ReadFile(filename string) ([]byte, error) {
	return os.ReadFile(filename)
}

//go:fix inline
func WriteFile(filename string, data []byte, perm os.FileMode) error {
	return os.WriteFile(filename, data, perm)
}

//go:fix inline
func ReadAll(r io.Reader) ([]byte, error) {
	return io.ReadAll(r)
}

//go:fix inline
func NopCloser(r io.Reader) io.ReadCloser {
	return io.NopCloser(r)
}

//go:fix inline
func TempFile(dir, pattern string) (*os.File, error) {
	return os.CreateTemp(dir, pattern)
}

// ReadDir carries no directive: its result type differs from os.ReadDir's, so
// it is not a forwarder and upstream says nothing about a call of it.
func ReadDir(dirname string) ([]os.FileInfo, error) {
	return nil, nil
}

// Discard is a variable, not a function.
var Discard io.Writer = io.Discard
