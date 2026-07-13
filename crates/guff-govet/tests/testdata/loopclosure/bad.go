package p

func f() {
	for _, v := range []int{1} {
		go func() {
			_ = v
		}()
	}
}
