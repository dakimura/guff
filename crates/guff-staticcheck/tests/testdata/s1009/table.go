package main

// Upstream's `isConstZero` answers *two* questions — is the bound a constant at
// all, and is it zero — and only the second one picks which comparison keeps
// the finding. guff's port folded both into one `Option<bool>` where `None`
// meant "not a constant", and the literal arm fell through to it for every
// literal that was not `0`. So the whole right-hand column below was dropped
// before the operator table was ever consulted, and the existing fixture could
// not see it: all four of its bounds are `0`.
//
// beats' `metricbeat/module/jolokia/jmx/config.go:210` writes
// `if (prop == nil) || (len(prop) < 3)`.
//
// Every line is marked from a measurement against golangci-lint 2.12.2.

const zero = 0
const three = 3

func table(s []int, n int) []bool {
	return []bool{
		// `x == nil || …`
		s == nil || len(s) == 0,     // FINDING
		s == nil || len(s) == 3,     // silent — a non-zero length is not empty
		s == nil || len(s) == zero,  // FINDING — a named zero constant
		s == nil || len(s) == three, // silent
		s == nil || len(s) <= 0,     // FINDING
		s == nil || len(s) <= 3,     // FINDING — `<=` never cares
		s == nil || len(s) < 3,      // FINDING
		s == nil || len(s) < zero,   // silent — `< 0` is never true
		s == nil || len(s) < three,  // FINDING
		s == nil || len(s) > 0,      // silent — the wrong direction
		s == nil || len(s) != 0,     // silent
		s == nil || len(s) < n,      // silent — not a constant

		// `x != nil && …`
		s != nil && len(s) != 0,     // FINDING
		s != nil && len(s) != 3,     // silent
		s != nil && len(s) == 0,     // silent
		s != nil && len(s) == 3,     // FINDING
		s != nil && len(s) == three, // FINDING
		s != nil && len(s) > 0,      // FINDING — `>` never cares
		s != nil && len(s) > 3,      // FINDING
		s != nil && len(s) >= 0,     // silent — `>= 0` is always true
		s != nil && len(s) >= 3,     // FINDING
		s != nil && len(s) <= 3,     // silent
		s != nil && len(s) > n,      // silent — not a constant
	}
}

// The message names the type, and only the three nil-able ones qualify.
func types(m map[string]int, c chan int) []bool {
	return []bool{
		m == nil || len(m) < 3, // FINDING — nil maps
		c == nil || len(c) < 3, // FINDING — nil channels
	}
}

// Parenthesised operands, which `pattern.match` strips and this port has to
// `unparen` by hand — beats writes the nil check and the length check in
// brackets.
func parens(s []int) []bool {
	return []bool{
		(s == nil) || (len(s) < 3), // FINDING
		(s == nil) || len(s) < 3,   // FINDING
		s == nil || (len(s) < 3),   // FINDING
		(s == nil) || (len(s) < 3), // FINDING
	}
}

func mk() []int { return nil }

// The nil-checked expression has to be free of side effects.
func sideEffects() bool {
	return mk() == nil || len(mk()) < 3 // silent
}
