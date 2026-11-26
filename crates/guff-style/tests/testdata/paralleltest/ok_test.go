package paralleltest

import "testing"

func TestWithParallel(t *testing.T) {
	t.Parallel()
	t.Error("ok")
}

func TestRangeWithParallel(t *testing.T) {
	t.Parallel()
	tests := []struct{ name string }{{"a"}, {"b"}}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			t.Error(tt.name)
		})
	}
}

func TestSubtestsWithParallel(t *testing.T) {
	t.Parallel()
	t.Run("one", func(t *testing.T) {
		t.Parallel()
		t.Error("one")
	})
	t.Run("two", func(t *testing.T) {
		t.Parallel()
		t.Error("two")
	})
}

func TestSetenvSkipsParallel(t *testing.T) {
	t.Setenv("K", "V")
	t.Error("setenv")
}
