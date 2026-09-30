package v3

const hdrA = "a"

func helper(xs []int) int {
	total := 0
	for i, x := range xs {
		if x > 10 {
			total += x * i
		} else if x < 0 {
			total -= x
		} else {
			total++
		}
	}
	return total
}
