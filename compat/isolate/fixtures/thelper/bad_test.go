package p

import "testing"

func helper(t *testing.T) {
	t.Fatal("x")
}

func TestBad(t *testing.T) {
	helper(t)
}
