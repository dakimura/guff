package dogsledok

func ret1() int { return 1 }
func ret2() (int, int) { return 1, 2 }

func Ok() {
	_ = ret1()
	_, _ = ret2()
}
