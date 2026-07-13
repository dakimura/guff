package bad

func f() {}

func bad() {
	_ = f == nil
}
