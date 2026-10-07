package a_test

import (
	"testing"

	"example.com/a"
)

func TestK(t *testing.T) {
	print(a.K) // not inlined: external test of the same package
}

func Test(t *testing.T) {
	print(a.K) // inlined
	var _ a.A  // inlined
}
