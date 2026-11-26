// Shapes of gocognit v1.3.0's syntactic recursion resolver (RecursiveFuncs
// with no type information, one file at a time) that its own testdata only
// meets through the type-aware Analyzer, plus the //gocognit:ignore directive.
package g

type T struct{ next *T }

// A and B call each other: one strongly connected component, +1 each.
func A(n int) int {
	if n == 0 {
		return 0
	}
	return B(n - 1)
}

func B(n int) int {
	if n == 0 {
		return 1
	}
	return A(n - 1)
}

// Several self-calls still add one increment.
func Fib(n int) int {
	if n < 2 {
		return n
	}
	return Fib(n-1) + Fib(n-2)
}

// Through the method's own receiver.
func (t *T) M() int {
	if t.next == nil {
		return 0
	}
	return t.M() + t.next.M()
}

// Through a different receiver variable only: not recognized.
func (t *T) N(o *T) int {
	if o == nil {
		return 0
	}
	return o.N(nil)
}

// A generic instantiation, parenthesised.
func G[E any](xs []E) int {
	if len(xs) == 0 {
		return 0
	}
	return (G[E])(xs[1:]) + 1
}

// Inside a closure.
func Lit(n int) int {
	f := func() int {
		if n == 0 {
			return 0
		}
		return Lit(n - 1)
	}
	return f()
}

// A local shadows the function's name: not a recursive call.
func Self(n int) int {
	Self := func(int) int { return 0 }
	if n > 0 {
		return Self(n)
	}
	return 0
}

//gocognit:ignore
func Ign(n int) int {
	if n > 0 {
		if n > 1 {
			return Ign(n - 1)
		}
	}
	return 0
}

// The else block is nested; parentheses continue a sequence.
func Else(a, b, c bool) int {
	if a && (b && c) {
		return 1
	} else {
		if b || (c || a) {
			return 2
		}
	}
	return 0
}
