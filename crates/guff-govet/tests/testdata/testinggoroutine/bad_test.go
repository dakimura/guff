package testinggoroutine

import "testing"

func TestGoroutine(t *testing.T) {
	// go t.Fatal(): the go statement itself is the region.
	go t.Fatal("no")

	// A literal: the report lands on the offending call, not on `go`.
	go func() {
		t.Fatalf("no %d", 1)
	}()

	// A variable holding a literal: the literal is the region, and the report
	// names the identifier that reached it.
	fn := func() {
		t.FailNow()
	}
	go fn()

	// A function declared in this package: its declaration is the region.
	go helper(t)

	// Every forbidden method, from a plain literal.
	go func() {
		t.Skip("a")
		t.Skipf("b %d", 2)
		t.SkipNow()
	}()
}

func helper(t *testing.T) {
	t.Fatal("from a helper")
}

func BenchmarkGoroutine(b *testing.B) {
	go b.Fatal("also forbidden on B")
}
