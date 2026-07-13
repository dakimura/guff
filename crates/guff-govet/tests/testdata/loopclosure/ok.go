package p

func f() {
	for _, v := range []int{1} {
		v := v
		go func() {
			_ = v
		}()
	}
}
