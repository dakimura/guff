package p

import "testing"

func TestBad(t *testing.T) {
	t.Run("sub", func(t *testing.T) {
		_ = 1
	})
}
