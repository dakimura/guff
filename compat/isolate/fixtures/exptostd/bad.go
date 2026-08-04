package p

import "golang.org/x/exp/maps"

func Bad(m map[string]string) {
	_ = maps.Clone(m)
}
