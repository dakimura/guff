package main

import "golang.org/x/exp/maps"

func f(m map[string]int) map[string]int {
	return maps.Clone(m)
}
