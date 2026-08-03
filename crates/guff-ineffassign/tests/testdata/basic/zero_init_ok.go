package zeroinit

type EnumItemIndex int

func f(n int) {
	state := EnumItemIndex(0)
	for i := 0; i < n; i++ {
		if i%2 == 0 {
			state = 1
		} else {
			state = 2
		}
		_ = state
	}
}

func g() {
	x := int(0)
	x = 1
	_ = x
}

func h() {
	x := 0
	x = 1
	_ = x
}
