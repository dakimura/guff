package main

import "sort"

func f(x []int) {
	sort.Sort(sort.IntSlice(x))
}

func g(x []string) {
	sort.Sort(sort.StringSlice(x))
}
