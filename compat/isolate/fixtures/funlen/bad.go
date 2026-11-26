package p

// funlen has two messages: one counts lines, one counts statements, and a
// function can trip either without the other. The line counter ignores blank
// lines and comments, so the first function below is long without being busy.
func TooLong() {
	println(
		0,
		1,
		2,
		3,
		4,
		5,
		6,
		7,
		8,
		9,
		10,
		11,
	)
}

func TooManyStatements() {
	a0 := 0
	_ = a0
	a1 := 1
	_ = a1
	a2 := 2
	_ = a2
	a3 := 3
	_ = a3
	a4 := 4
	_ = a4
	a5 := 5
	_ = a5
	a6 := 6
	_ = a6
	a7 := 7
	_ = a7
	a8 := 8
	_ = a8
	a9 := 9
	_ = a9
	a10 := 10
	_ = a10
	a11 := 11
	_ = a11
}

// Short / empty bodies must not be flagged (regression: i64 underflow →
// usize::MAX). Restored after being dropped while widening — the two counter
// arms above are additions, not a replacement.
func OneLine() string { return "ok" }

func Empty() {}
