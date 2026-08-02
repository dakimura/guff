package pkg

func gen1() int { return 0 }

func fn() {
	var a = gen1()
	var b int
	b = gen1()
	// Explicit int64 is required: bare `-1` defaults to int, not int64.
	var originalQuota int64 = -1
	var alsoNeeded int64 = +1
	_, _, _ = a, b, originalQuota
	_ = alsoNeeded
}
