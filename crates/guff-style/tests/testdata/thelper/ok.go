package thelperok

import (
	"context"
	"testing"
)

func helperWithHelper(t *testing.T) {
	t.Helper()
}

func helperParamSecondWithContext(ctx context.Context, t *testing.T) {
	t.Helper()
}

func helperWithNoName(_ *testing.T) {}

func TestSomething(t *testing.T) {
	// entry points are skipped
}

func BenchmarkSomething(b *testing.B) {}

func TestSubtestAnonymous(t *testing.T) {
	t.Run("sub", func(t *testing.T) {})
}

func onlyAsSubtest(t *testing.T) {}

func TestCallsOnlyAsSubtest(t *testing.T) {
	t.Run("sub", onlyAsSubtest)
}
