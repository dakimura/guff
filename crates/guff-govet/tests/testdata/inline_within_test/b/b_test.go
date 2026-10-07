package b_test

import (
	"testing"

	"example.com/a"
)

func TestK(t *testing.T) {
	print(a.K) // inlined: another package's test
}
