package main

type holder struct{ items []string }

type tags map[string][]string

func makeSrc() []string { return nil }

// The destination is bound once and recalled, so the two occurrences only have
// to be the *same expression*. Everything below a plain identifier was
// invisible while that comparison was on AST node ids; boundary writes one of
// each of the next three.

func plainIdentifier(src []string) []string {
	var dst []string
	for _, v := range src {
		dst = append(dst, v)
	}
	return dst
}

func selector(h *holder, src []string) {
	for _, v := range src {
		h.items = append(h.items, v)
	}
}

func index(m map[string][]string, k string, src []string) {
	for _, v := range src {
		m[k] = append(m[k], v)
	}
}

func indexThroughAParenthesisedDeref(t *tags, k string, src []string) {
	for _, v := range src {
		(*t)[k] = append((*t)[k], v)
	}
}

// A value-based loop evaluates x once, so an impure x is fine here.
func rangingOverACall(h *holder) {
	for _, v := range makeSrc() {
		h.items = append(h.items, v)
	}
}

// The index-based arm, on a pure operand.
func indexBasedLoop(src []string) []string {
	var dst []string
	for i := range src {
		dst = append(dst, src[i])
	}
	return dst
}

// The index-based arm with the intermediate assignment, and a selector.
func indexBasedLoopWithATemporary(src []string, h *holder) {
	for i := range src {
		v := src[i]
		h.items = append(h.items, v)
	}
}
