package main

import "sort"

type T3 [1]int
type T4 string

func less(i, j int) bool { return false }

func main() {
	var v3 T3
	var v4 T4
	sort.Slice(v3, less)
	sort.Slice(v4, less)
	sort.Slice(0, less)
}
