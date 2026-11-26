package p

import "testing"

func TestName(t *testing.T) {
	if Name() != "p" || Size() != 1 {
		t.Fatal("unexpected")
	}
}
