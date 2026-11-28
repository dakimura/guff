package last

import "testing"

func use(...any) {}

// The "last statement" is recursive: every branch of a trailing if / else-if /
// else chain, loop, switch, type switch and select is a last statement.
func lastShapes(xs []int, ch chan int, a any) {
	for i := range xs {
		if i > 0 {
			go func() { use(i) }()
		} else if i < 0 {
			go func() { use(i) }()
		} else if i == 1 {
			defer func() { use(i) }()
		} else {
			go func() { use(i) }()
		}
	}
	for i := 0; i < 3; i++ {
		for range xs {
			go func() { use(i) }()
		}
	}
	for i, v := range xs {
		switch v {
		case 1:
			go func() { use(i) }()
		default:
			use(v)
			go func() { use(v) }()
		}
	}
	for i := range xs {
		switch a.(type) {
		case int:
			defer func() { use(i) }()
		}
	}
	for i := range xs {
		select {
		case <-ch:
			go func() { use(i) }()
		}
	}
	// Not last: followed by another statement.
	for i := range xs {
		go func() { use(i) }()
		use(i)
	}
	// Parenthesised literal and parenthesised key: upstream asserts the bare
	// node type, so neither is checked.
	for i := range xs {
		go (func() { use(i) })()
	}
	var k int
	for (k) = range xs {
		go func() { use(k) }()
	}
	// Assignment post statement: p is a loop variable.
	type node struct{ next *node }
	for p := (*node)(nil); p != nil; p = p.next {
		go func() { use(p) }()
	}
}

func TestSubtests(t *testing.T) {
	for i, v := range []int{1, 2} {
		t.Run("a", func(t *testing.T) {
			use(i)
			t.Parallel()
			use(v)
		})
		t.Run("b", func(t *testing.T) {
			t.Parallel()
		L:
			use(i)
			use(v)
			goto L
		})
		t.Run("c", func(t *testing.T) {
			t.Parallel()
			use(i)
		})
		t.Run("d", func(u *testing.T) {
			t.Parallel()
			use(i)
		})
		use(v)
	}
}
