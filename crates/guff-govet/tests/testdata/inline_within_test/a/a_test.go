package a

import "testing"

// x_test.go next to x.go: every use of x.go's symbols is the test's own.
func TestWithAnyName(t *testing.T) {
	print(K) // not inlined
	var _ A  // not inlined
}
