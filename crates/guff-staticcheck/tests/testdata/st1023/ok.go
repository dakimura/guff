package pkg

func gen1() int { return 0 }

func fn() {
	var a = gen1()
	var b int
	b = gen1()
	_, _ = a, b
}
