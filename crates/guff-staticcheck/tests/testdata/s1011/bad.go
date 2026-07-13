package main

func f(x, y []int) {
	for _, e := range y {
		x = append(x, e)
	}
}
