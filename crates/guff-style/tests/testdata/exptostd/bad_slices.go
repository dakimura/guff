package exptostd

import "golang.org/x/exp/slices"

func use(a, b []string) {
	_ = slices.Equal(a, b)
	_ = slices.Clone(a)
	slices.Sort(a)
	slices.Reverse(a)
}
