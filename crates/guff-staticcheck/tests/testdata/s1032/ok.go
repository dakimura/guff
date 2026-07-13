package main

import "sort"

func f(x []int) {
	sort.Ints(x)
}

func g(x []int) {
	var e sort.Interface
	sort.Sort(e)
	sort.Sort(sort.IntSlice(x))
}
