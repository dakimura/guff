package copyloopvar

func Ok() {
	for i, v := range []int{1, 2, 3} {
		_i := i
		_v := v
		_, _ = _i, _v
	}

	for i := 0; i < 3; i++ {
		_i := i
		_ = _i
	}
}
