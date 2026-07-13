package main

import "sort"

type T1 []int

func less(i, j int) bool { return false }

func main() {
	var v1 T1
	var v5 []int
	sort.Slice(v1, less)
	sort.Slice(v5, less)
	sort.Slice([]int{}, less)
}
