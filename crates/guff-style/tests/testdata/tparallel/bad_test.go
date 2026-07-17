package tparallel

import "testing"

func call(_ string) {}

func setup(_ string) func() {
	return func() {}
}

func Test_Func1(t *testing.T) {
	teardown := setup("Test_Func1")
	t.Cleanup(teardown)

	t.Run("Func1_Sub1", func(t *testing.T) {
		call("Func1_Sub1")
		t.Parallel()
	})

	t.Run("Func1_Sub2", func(t *testing.T) {
		call("Func1_Sub2")
		t.Parallel()
	})
}

func Test_Func2(t *testing.T) {
	teardown := setup("Test_Func2")
	t.Cleanup(teardown)
	t.Parallel()

	t.Run("Func2_Sub1", func(t *testing.T) {
		call("Func2_Sub1")
	})

	t.Run("Func2_Sub2", func(t *testing.T) {
		call("Func2_Sub2")
	})
}

func Test_Cleanup1(t *testing.T) {
	teardown := setup("Test_Cleanup1")
	defer teardown()

	t.Parallel()

	t.Run("Cleanup1_Sub1", func(t *testing.T) {
		t.Parallel()
		call("Cleanup1_Sub1")
	})

	t.Run("Cleanup1_Sub2", func(t *testing.T) {
		call("Cleanup1_Sub2")
	})
}

func Test_Table1(t *testing.T) {
	teardown := setup("Test_Table1")
	defer teardown()

	tests := []struct {
		name string
	}{
		{name: "Table1_Sub1"},
		{name: "Table1_Sub2"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			call(tt.name)
		})
	}
}

func Test_Table2(t *testing.T) {
	teardown := setup("Test_Table2")
	t.Cleanup(teardown)
	t.Parallel()

	tests := []struct {
		name string
	}{
		{name: "Table2_Sub1"},
		{name: "Table2_Sub2"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			call(tt.name)
		})
	}
}

func namedSub(t *testing.T) {
	t.Parallel()
	call("named")
}

func Test_NamedSub(t *testing.T) {
	t.Cleanup(func() {})
	t.Run("sub", namedSub)
}
