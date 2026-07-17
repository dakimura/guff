package tparallel_ok

import "testing"

func call(_ string) {}

func setup(_ string) func() {
	return func() {}
}

func Test_Func3(t *testing.T) {
	teardown := setup("Test_Func3")
	t.Cleanup(teardown)
	t.Parallel()

	t.Run("Func3_Sub1", func(t *testing.T) {
		call("Func3_Sub1")
		t.Parallel()
	})

	t.Run("Func3_Sub2", func(t *testing.T) {
		call("Func3_Sub2")
		t.Parallel()
	})
}

func Test_Func4(t *testing.T) {
	teardown := setup("Test_Func4")
	defer teardown()
}

func Test_Cleanup2(t *testing.T) {
	teardown := setup("Test_Cleanup2")
	defer teardown()

	t.Run("Cleanup2_Sub1", func(t *testing.T) {
		call("Cleanup2_Sub1")
	})

	t.Run("Cleanup2_Sub2", func(t *testing.T) {
		call("Cleanup2_Sub2")
	})
}

func Test_Cleanup3(t *testing.T) {
	t.Parallel()
	call("Test_Cleanup3")

	t.Run("Cleanup3_Sub1", func(t *testing.T) {
		t.Parallel()
		call("Cleanup3_Sub1")
	})

	t.Run("Cleanup3_Sub2", func(t *testing.T) {
		t.Parallel()
		call("Cleanup3_Sub2")
	})
}

func Test_Table3(t *testing.T) {
	teardown := setup("Test_Table3")
	t.Cleanup(teardown)
	t.Parallel()

	tests := []struct {
		name string
	}{
		{name: "Table3_Sub1"},
		{name: "Table3_Sub2"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			call(tt.name)
		})
	}
}

func TestWithoutSub(t *testing.T) {
	t.Parallel()
	call("TestWithoutSub")
}

func namedOk(t *testing.T) {
	t.Parallel()
	call("named")
}

func Test_NamedOk(t *testing.T) {
	t.Cleanup(func() {})
	t.Parallel()
	t.Run("sub", namedOk)
}
