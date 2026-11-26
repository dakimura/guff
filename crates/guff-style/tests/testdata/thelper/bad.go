package thelper

import (
	"context"
	"testing"
)

func helperWithoutHelper(t *testing.T) {} // want begin

func helperWithHelper(t *testing.T) {
	t.Helper()
}

func helperWithHelperAfterAssignment(t *testing.T) { // want begin
	_ = 0
	t.Helper()
}

func helperParamNotFirst(s string, t *testing.T) { // want first
	t.Helper()
}

func helperParamSecondWithContext(ctx context.Context, t *testing.T) {
	t.Helper()
}

func helperWithIncorrectName(o *testing.T) { // want name
	o.Helper()
}

func helperWithNoName(_ *testing.T) {}

func bhelperWithoutHelper(b *testing.B) {} // want begin

func bhelperWithIncorrectName(o *testing.B) { // want name
	o.Helper()
}

func tbhelperWithoutHelper(tb testing.TB) {} // want begin

func tbhelperWithIncorrectName(o testing.TB) { // want name
	o.Helper()
}

func TestSomething(t *testing.T) {
	t.Helper() // Test* skipped
}

func TestSubtestShouldNotBeChecked(t *testing.T) {
	t.Run("sub", func(t *testing.T) {
		t.Parallel()
		t.Error("test")
	})
}

func check(t *testing.T) {
	anotherCheck(t)
}

func anotherCheck(t *testing.T) {} // want begin — also called from check

func TestSubtest(t *testing.T) {
	t.Run("sub", check)
}
