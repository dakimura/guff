package gosec_g602_variadic

// Where a slice's capacity comes from when guff's SSA does not spell it the way
// upstream reads it. Upstream's G602 only ever learns a capacity from an
// `Alloc` of a fixed-size array; guff builds no such array for a variadic call
// and none for `make([]T, constN)`, so a whole family of findings never
// started.
//
// Every function below was run through golangci-lint 2.12.2 (gosec v2.26.1) and
// guff side by side. `// FINDING` marks the ones upstream reports, `// silent`
// the ones it does not, and the test pins the report positions — the message is
// the same string for all of them, so counting substrings would pass with any
// subset.
//
// Note when re-measuring by hand: golangci-lint's default `max-same-issues: 3`
// caps identical messages, and every finding here has the *same* message. Three
// of these show up per run, chosen nondeterministically, unless the config sets
// `issues.max-same-issues: 0`.

// --- what already worked: the capacity comes from an array in this function --

func localLiteralIndexed() any { // FINDING
	pairs := []any{"a", 1}
	var last any
	for i := 0; i < len(pairs); i += 2 {
		last = pairs[i+1]
	}
	return last
}

func localLiteralGuarded() any { // FINDING
	pairs := []any{"a", 1}
	var last any
	p := len(pairs)
	for i := 0; i < p; i += 2 {
		if i+1 >= p {
			continue
		}
		last = pairs[i+1]
	}
	return last
}

func literalThroughCall(pairs []any) any { // FINDING at the index below
	var last any
	p := len(pairs)
	for i := 0; i < p; i += 2 {
		if i+1 >= p {
			continue
		}
		last = pairs[i+1]
	}
	return last
}

func callsLiteralThroughCall() { _ = literalThroughCall([]any{"a", 1}) }

func spreadThroughCall(pairs ...any) any { // FINDING at the index below
	var last any
	for i := 0; i < len(pairs); i += 2 {
		last = pairs[i+1]
	}
	return last
}

func callsSpreadThroughCall() {
	s := []any{"a", 1}
	_ = spreadThroughCall(s...)
}

// --- make([]T, constN) handed to a function -------------------------------
//
// go/ssa lowers a constant `make` to `Alloc *[N]T` + `Slice`, so upstream walks
// into the callee. guff emits one MakeSlice, and the `Call → parameter` step
// only accepted a `Slice`.

func makeThroughCall(pairs []any) any { // FINDING at the index below
	var last any
	for i := 0; i < len(pairs); i += 2 {
		last = pairs[i+1]
	}
	return last
}

func callsMakeThroughCall() { _ = makeThroughCall(make([]any, 2)) }

// --- the variadic tail -----------------------------------------------------
//
// go/ssa packs `f(a, b)` into a fresh `Alloc *[2]any`; guff passes the tail
// through individually, so the capacity has to be read off the call site.

func variadicPlain(pairs ...any) any { // FINDING at the index below
	var last any
	for i := 0; i < len(pairs); i += 2 {
		last = pairs[i+1]
	}
	return last
}

func callsVariadicPlain() { _ = variadicPlain("a", 1) }

func variadicGuardedContinue(pairs ...any) any { // FINDING at the index below
	var last any
	p := len(pairs)
	for i := 0; i < p; i += 2 {
		if i+1 >= p {
			continue
		}
		last = pairs[i+1]
	}
	return last
}

func callsVariadicGuardedContinue() { _ = variadicGuardedContinue("a", 1) }

// The guard around the access rather than before it. This is the shape that
// decides which comparison against `len(pairs)` the `ifs` map records: `i < p`
// (offset 0, keeps the finding) or `i+1 < p` (offset -1, deletes it). go/ssa
// lists referrers in build order and offers `i < p` first; guff's block arena
// holds the body before the condition and offered the other one.
func variadicGuardedIf(pairs ...any) any { // FINDING at the index below
	var last any
	p := len(pairs)
	for i := 0; i < p; i += 2 {
		if i+1 < p {
			last = pairs[i+1]
		}
	}
	return last
}

func callsVariadicGuardedIf() { _ = variadicGuardedIf("a", 1) }

func variadicOddArgCount(pairs ...any) any { // FINDING at the index below
	var last any
	for i := 0; i < len(pairs); i += 2 {
		last = pairs[i+1]
	}
	return last
}

func callsVariadicOddArgCount() { _ = variadicOddArgCount("a", 1, "b") }

func variadicConstIndexBad(pairs ...any) any { return pairs[3] } // FINDING

func callsVariadicConstIndexBad() { _ = variadicConstIndexBad("a", 1) }

func variadicConstIndexOK(pairs ...any) any { return pairs[1] } // silent: 1 < 2

func callsVariadicConstIndexOK() { _ = variadicConstIndexOK("a", 1) }

func variadicStepOne(pairs ...any) any { // FINDING at the index below
	var last any
	for i := 0; i < len(pairs); i++ {
		last = pairs[i+1]
	}
	return last
}

func callsVariadicStepOne() { _ = variadicStepOne("a", 1) }

// A method: the receiver occupies args[0] and params[0] alike, so the tail is
// still counted from the end.
type holder struct{}

func (h *holder) variadicMethod(pairs ...any) any { return pairs[3] } // FINDING

func callsVariadicMethod() { _ = (&holder{}).variadicMethod("a", 1) }

// Two call sites; the shorter one is what makes the index bad.
func variadicTwoCallSites(pairs ...any) any { return pairs[2] } // FINDING

func callsVariadicLong() { _ = variadicTwoCallSites("a", 1, "b", 2) }

func callsVariadicShort() { _ = variadicTwoCallSites("a", 1) }

// The tail is re-sliced before being indexed: cap 2, `[1:]` leaves 1.
func variadicResliced(pairs ...any) any {
	rest := pairs[1:]
	return rest[2] // FINDING
}

func callsVariadicResliced() { _ = variadicResliced("a", 1) }

// --- where the synthesis stops, each one measured --------------------------

// An empty tail: go/ssa passes the `nil` slice constant, not an `Alloc`, so
// upstream never learns a capacity — even though it is 0 and `pairs[0]` is out
// of range in every execution.
func variadicNoArgs(pairs ...any) any { return pairs[0] } // silent

func callsVariadicNoArgs() { _ = variadicNoArgs() }

// A call through a function value: upstream needs `Call.Value.(*ssa.Function)`.
func variadicViaValue(pairs ...any) any { return pairs[3] } // silent

var viaValue = variadicViaValue

func callsVariadicViaValue() { _ = viaValue("a", 1) }

// The tail forwarded on with `...`: upstream's `Call → parameter` step matches
// only an argument that *is* the tracked slice value, and a parameter is not,
// so the walk stops at one hop in both tools.
func forwardInner(pairs ...any) any { return pairs[3] } // silent

func forwardOuter(pairs ...any) any { return forwardInner(pairs...) }

func callsForwardOuter() { _ = forwardOuter("a", 1) }

// The same one-hop stop with a plain slice parameter.
func handInner(pairs []any) any { return pairs[3] } // silent

func handOuter(pairs ...any) any { return handInner(pairs) }

func callsHandOuter() { _ = handOuter("a", 1) }

// A wrapper nobody calls: there is no call site, so there is no capacity.
func variadicNeverCalled(pairs ...any) any { return pairs[3] } // silent

// The tail is only ranged over.
func variadicRanged(pairs ...any) int { // silent
	n := 0
	for range pairs {
		n++
	}
	return n
}

func callsVariadicRanged() { _ = variadicRanged("a", 1) }

// Index at offset 0 with an even step: never past the end.
func variadicIndexAtI(pairs ...any) any { // silent
	var last any
	for i := 0; i < len(pairs); i += 2 {
		last = pairs[i]
	}
	return last
}

func callsVariadicIndexAtI() { _ = variadicIndexAtI("a", 1) }
