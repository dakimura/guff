package paralleltest

import "testing"

func TestMissingParallel(t *testing.T) {
	t.Error("no parallel")
}

func TestRangeMissingParallel(t *testing.T) {
	t.Parallel()
	tests := []struct{ name string }{{"a"}, {"b"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Error(tt.name)
		})
	}
}

func TestSubtestsMissingParallel(t *testing.T) {
	t.Parallel()
	t.Run("one", func(t *testing.T) {
		t.Error("one")
	})
	t.Run("two", func(t *testing.T) {
		t.Error("two")
	})
}
