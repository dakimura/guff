package testinggoroutine

import "testing"

func TestOK(t *testing.T) {
	// The test's own goroutine may call any of them.
	if false {
		t.Fatal("fine")
	}

	// Error/Errorf do not call runtime.Goexit, so they are fine anywhere.
	go func() {
		t.Errorf("fine %d", 1)
	}()

	// A subtest's own t, inside the subtest: upstream's -subtest reporting is
	// off, and the receiver is declared inside the region either way.
	t.Run("sub", func(t *testing.T) {
		t.Fatal("fine")
	})

	// A t.Fatal inside a subtest nested in a goroutine belongs to the subtest
	// region, so it is not a "non-test goroutine" call.
	go func() {
		t.Run("nested", func(t *testing.T) {
			t.Fatal("fine")
		})
	}()
}

func notATest() {
	// No *testing.T parameter: this function is not walked at all.
	var t *testing.T
	go func() {
		t.Fatal("not reported")
	}()
}
