// The gocritic checks whose `settings` entry guff used to ignore: each param
// was a constant at its go-critic default, so a config moving it changed
// nothing. `gocritic-check-flags-default` and `-tuned` run this file with the
// defaults and with every param moved.
package gocritic

func cfCommented(total int) int {
	// total = total + 1
	total++
	// total = total + computeSomethingExpensive(total, 42)
	// fmt.Println(x)
	return total
}

// captLocal: params, results and the receiver are checked by default; with
// `paramsOnly: false` the body's `:=`, `var` and `const` names are too — but
// not a function literal on the right of `:=`, not a range variable, and not a
// name `:=` merely re-assigns.
type cfT struct{ field int }

func (R *cfT) cfReceiver() {}

func cfParams(In int) (Out int) {
	X := In
	var Y = 2
	const Z = 3
	var A, B = cfPair()
	X, W := cfPair()
	F := func() {
		G := 1
		_ = G
	}
	for K := range []int{X} {
		_ = K
	}
	F()
	return X + Y + Z + A + B + W
}

func cfPair() (int, int) { return 1, 2 }

// elseif: a "balanced" pair — the then-body is itself a lone `if` — is skipped
// by default (`skipBalanced: true`).
func cfElseif(a, b, c bool) int {
	if a {
		if b {
			return 1
		}
	} else {
		if c {
			return 2
		}
	}
	if a {
		return 3
	} else {
		if c {
			return 4
		}
	}
	return 0
}

// underef: `(*p).M()` on a pointer-receiver method is skipped by default
// (`skipRecvDeref: true`); a field read is reported either way.
func (t *cfT) cfPtrMethod() int { return t.field }

func cfUnderef(p *cfT) int {
	return (*p).cfPtrMethod() + (*p).field
}
