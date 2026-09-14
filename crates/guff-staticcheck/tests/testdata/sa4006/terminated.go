// SA4006 asks the IR whether a value has referrers; guff's port then vetoes
// the answer when the *source* reads the object further down, because its SSA
// loses a use in two shapes (a read on the redefining statement's own
// right-hand side, and a loop back edge).
//
// Both of those reach the value through a later execution. When a `return`
// follows the assignment in its own statement list there is no later
// execution, so the veto has to stand down — otherwise the only read being a
// `range` header *above* the assignment reads as "used later" and the finding
// disappears. opentofu's `internal/legacy/tofu/state.go:442` is that shape.
package main

// fires — the value assigned to `is` dies at the `return`; the only other
// mention of `is` is the range header above it.
func removeFromLists(lists [][]*int, v *int) {
	for _, is := range lists {
		for i, instance := range is {
			if instance == v {
				is, is[len(is)-1] = append(is[:i], is[i+1:]...), nil

				return
			}
		}
	}
}

// fires — the same without the tuple assignment, in an `if` rather than a
// loop.
func storeThenReturn(a int) int {
	x := a
	if a > 0 {
		x = a + 1

		return 0
	}

	return x
}

// silent — the read sits *between* the assignment and the `return` that ends
// the statement list. The first version of this rule dropped the veto on the
// strength of the `return` alone and made VictoriaMetrics'
// `app/vmselect/graphite/tags_api.go:94` — `deadline := …` followed by
// `_ = deadline // TODO` and, much later, a `return` — a finding.
func storeReadBeforeTheReturn(n int) error {
	deadline := compute(n)
	_ = deadline // TODO: use it

	return nil
}

func compute(n int) int { return n }

// silent — the assignment is read before the return.
func storeReadThenReturn(a int) int {
	x := a
	if a > 0 {
		x = a + 1
		_ = x

		return x
	}

	return x
}

// silent — everything after a call that cannot return is a block upstream's IR
// never builds, so the assignment it holds is not an assignment either. This is
// VictoriaMetrics `lib/fs/reader_at.go:312`, where the CAS loop sits behind
// `if !mincore(…)` and `mincore` is `panic(…)` on every non-linux build. Turn
// `mustPanic` into something that returns and the finding comes back — which is
// what the `reported` twin below pins.
func behindANoReturnCall(xs []int, v int) int {
	n := 0
	for _, e := range xs {
		if mustPanic(e) {
			return 0
		}

		if e == v {
			n = e

			return 1
		}
	}

	return n
}

func mustPanic(int) bool { panic("BUG: unexpected call") }

type dec struct{ n int }

func (d dec) Skip(string) dec { return d }

// silent — the redefining statement reads the value on its own right-hand
// side. This is one of the two shapes the veto exists for.
func readOnOwnRhs(d dec) dec {
	decoder := d
	decoder = decoder.Skip("type_url")

	return decoder
}

// silent — the other one: the loop back edge carries the value to a read that
// appears earlier in the source.
func readAcrossBackEdge(xs []int) int {
	acc := 0
	for _, x := range xs {
		acc = acc + x
	}

	return acc
}
