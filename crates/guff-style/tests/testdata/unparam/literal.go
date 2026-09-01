package unparam

// A variadic parameter and a func literal are the two shapes `bad.go` has none
// of. Both need at least four call sites before upstream will call a parameter
// constant, so every helper here is called five times from distinct statements.

type handler = func(c int) error

type named func(c int) error

type holder struct{ h handler }

var global handler

func register(h handler) { _ = h }

func registerAny(v any) { _ = v }

// neverGiven is never handed a variadic argument, so go/ssa's packed slice is
// a nil constant at every call site.
func neverGiven(s string, count ...int) string {
	if len(count) > 0 {
		return s
	}

	return s + "x"
}

// alwaysGiven is handed one every time: the packed slice is an `*ssa.Slice`,
// which is not a constant, so the parameter is not reported.
func alwaysGiven(s string, count ...int) string {
	if len(count) > 0 {
		return s
	}

	return s + "x"
}

// spread hands a slice through with `...`, which is not a constant either.
func spread(s string, count ...int) string {
	if len(count) > 0 {
		return s
	}

	return s + "x"
}

// spreadNil hands `nil` through with `...`, which *is* a constant.
func spreadNil(s string, count ...int) string {
	if len(count) > 0 {
		return s
	}

	return s + "x"
}

// mixedGiven is given the argument at some call sites and not others.
func mixedGiven(s string, count ...int) string {
	if len(count) > 0 {
		return s
	}

	return s + "x"
}

// onlyVariadic has nothing but the variadic parameter.
func onlyVariadic(count ...int) int {
	return len(count)
}

type box struct{ n int }

// tagged is a method: go/ssa's argument list includes the receiver and go/ast's
// does not, so the two indices differ by one.
func (b *box) tagged(tag string, count ...int) int {
	return b.n + len(count) + len(tag)
}

func callVariadic(b *box) {
	xs := []int{1}

	_ = neverGiven("a")
	_ = neverGiven("b")
	_ = neverGiven("c")
	_ = neverGiven("d")
	_ = neverGiven("e")

	_ = alwaysGiven("a", 1)
	_ = alwaysGiven("b", 1)
	_ = alwaysGiven("c", 1)
	_ = alwaysGiven("d", 1)
	_ = alwaysGiven("e", 1)

	_ = spread("a", xs...)
	_ = spread("b", xs...)
	_ = spread("c", xs...)
	_ = spread("d", xs...)
	_ = spread("e", xs...)

	_ = spreadNil("a", nil...)
	_ = spreadNil("b", nil...)
	_ = spreadNil("c", nil...)
	_ = spreadNil("d", nil...)
	_ = spreadNil("e", nil...)

	_ = mixedGiven("a")
	_ = mixedGiven("b")
	_ = mixedGiven("c", 1)
	_ = mixedGiven("d", 1)
	_ = mixedGiven("e")

	_ = onlyVariadic()
	_ = onlyVariadic()
	_ = onlyVariadic()
	_ = onlyVariadic()
	_ = onlyVariadic()

	_ = b.tagged("t")
	_ = b.tagged("t")
	_ = b.tagged("t")
	_ = b.tagged("t")
	_ = b.tagged("t")
}

// The func-literal shapes. Upstream pins a literal's signature only when it can
// follow the value back to the function; which of these it manages is the whole
// rule, and an assignment to a plain local is *not* one of them.

// litLocalThenCall: the value resolves at the call, so the signature is pinned.
func litLocalThenCall() {
	h := func(c int) error {
		_ = c

		return nil
	}
	register(h)
}

func litDirect() {
	register(func(c int) error {
		_ = c

		return nil
	})
}

func litIntoField(x *holder) {
	x.h = func(c int) error {
		_ = c

		return nil
	}
}

func litIntoElement(xs []handler) {
	xs[0] = func(c int) error {
		_ = c

		return nil
	}
}

func litIntoGlobal() {
	global = func(c int) error {
		_ = c

		return nil
	}
}

func litReturned() handler {
	return func(c int) error {
		_ = c

		return nil
	}
}

func litBoxed() {
	registerAny(func(c int) error {
		_ = c

		return nil
	})
}

func litConverted() {
	_ = named(func(c int) error {
		_ = c

		return nil
	})
}

func litInComposite() *holder {
	return &holder{h: func(c int) error {
		_ = c

		return nil
	}}
}

func litLocalThenInvoke() {
	h := func(c int) error {
		_ = c

		return nil
	}
	_ = h(1)
	_ = h(2)
}

// litCapturedFreeVar is fiber's shape. The literal captures a local, so the
// value stored into the cell is a closure rather than a bare function, and the
// cell is itself captured — upstream's free-variable map cannot resolve either
// hop, so the literal stays checkable and its always-nil result is reported.
func litCapturedFreeVar(n int) int {
	hits := 0
	h := func(c int) error {
		hits += c

		return nil
	}
	for range n {
		func() {
			register(h)
		}()
	}

	return hits
}

// litPlainFreeVar is the same with the capture removed: now the stored value is
// a bare function, the map resolves it, and the signature is pinned.
func litPlainFreeVar(n int) {
	h := func(c int) error {
		_ = c

		return nil
	}
	for range n {
		func() {
			register(h)
		}()
	}
}

// litLiveIIFE is the control for the pair below: an immediately-invoked
// literal with one parameter it never uses, which upstream reports.
func litLiveIIFE() {
	_ = func(used int, unused string) int { return used + 1 }(1, "x")
}

// litDeadIIFE is the same literal in a statement nothing reaches. go/ssa's
// builder only visits statements while it holds a current block, so the
// `MakeClosure` is never built, no `AnonFuncs` entry is appended, and
// `ssautil.AllFunctions` cannot reach the literal — upstream criticises
// neither its parameters nor its results, not even the unused one. guff builds
// the literal regardless.
func litDeadIIFE() {
	panic("nothing below this runs")

	_ = func(used int, unused string) int { return used + 1 }(1, "x")
}
