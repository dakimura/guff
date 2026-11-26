package p

import "golang.org/x/exp/maps"

func Bad(m map[string]string) {
	_ = maps.Clone(m)
}

// exptostd names the stdlib replacement, so each x/exp function is its own
// sentence.
func Keys(m map[string]string) {
	_ = maps.Keys(m)
}

func Values(m map[string]string) {
	_ = maps.Values(m)
}
