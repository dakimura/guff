package main

func f() {
	var x []int
	var p *[]int
	if x != nil && len(x) == 0 {
	}
	if p != nil && len(*p) != 0 {
	}
}
