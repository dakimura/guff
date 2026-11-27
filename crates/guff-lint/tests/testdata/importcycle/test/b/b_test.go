package b

import (
	"testing"

	"example.com/cycle/a"
)

func TestB(t *testing.T) {
	if a.A() != 1 {
		t.Fatal("a.A")
	}
}
