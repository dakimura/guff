package p

func Bad() {
	for i, v := range []int{1, 2, 3} {
		i := i
		v := v
		_, _ = i, v
	}
}

// copyloopvar names the variable, so each redundant copy is its own sentence,
// and the `for i := range n` form is a separate node from `range slice`.
func RangeInt() {
	for i := range 3 {
		i := i
		_ = i
	}
}

func ThreeClause() {
	for i := 0; i < 3; i++ {
		i := i
		_ = i
	}
}
