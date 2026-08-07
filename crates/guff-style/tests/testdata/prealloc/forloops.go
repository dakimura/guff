package prealloc

// Three-clause loops, only reported when `for-loops` is on. Expected capacities
// were taken from golangci-lint 2.12 with `prealloc.for-loops: true`.

func ForSimple(n int) []int {
	var out []int
	for i := 0; i < n; i++ {
		out = append(out, i)
	}
	return out
}

func ForInclusive(n int) []int {
	var out []int
	for i := 0; i <= n; i++ {
		out = append(out, i)
	}
	return out
}

func ForDown(n int) []int {
	var out []int
	for i := n; i > 0; i-- {
		out = append(out, i)
	}
	return out
}

func ForStep(n int) []int {
	var out []int
	for i := 0; i < n; i += 2 {
		out = append(out, i)
	}
	return out
}

func ForStepAssign(n, k int) []int {
	var out []int
	for i := 0; i < n; i = i + k {
		out = append(out, i)
	}
	return out
}

func ForMin(a, b, c int) []int {
	var out []int
	for i := 0; i < a && i < b && i < c; i++ {
		out = append(out, i)
	}
	return out
}

func ForMax(n, m int) []int {
	var out []int
	for i := 0; i < n || i < m; i++ {
		out = append(out, i)
	}
	return out
}

func ForFlipped(n int) []int {
	var out []int
	for i := 0; n > i; i++ {
		out = append(out, i)
	}
	return out
}

func ForNested(n int) []int {
	var out []int
	for i := 0; i < n; i++ {
		for j := 0; j < 3; j++ {
			out = append(out, i*j)
		}
	}
	return out
}

// Not reported at all: the `break` sets hasBranch, which excludes the loop
// before the trip count is even considered.
func ForNoPost(n int) []int {
	var out []int
	for i := 0; ; i++ {
		if i > n {
			break
		}
		out = append(out, i)
	}
	return out
}

// A call in the bound is not pure, so the count is indeterminate.
func ForImpureBound(n int) []int {
	var out []int
	for i := 0; i < impure(); i++ {
		out = append(out, i)
	}
	return out
}

func impure() int { return 0 }
