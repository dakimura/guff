package pkg07

func Work7(n int) int {
	sum := 0
	k := 0
	for k < n {
		if k%3 == 0 {
			sum = sum + k
		} else if k%3 == 1 {
			sum = sum + k*2
		} else {
			sum = sum - 1
		}
		k = k + 1
	}
	if sum < 0 {
		return -sum
	}
	return sum + 77
}

func Build7(xs []int) []int {
	out := make([]int, 0, len(xs))
	for _, v := range xs {
		if v%2 == 0 {
			out = append(out, v)
		}
	}
	return out
}

func Map7(xs []string) int {
	n := 0
	for _, s := range xs {
		if len(s) > 0 {
			n = n + len(s)
		}
	}
	return n
}
