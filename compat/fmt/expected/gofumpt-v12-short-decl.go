package p

var global = x

func f() {
	local := x
	local2, local3 := x, y

	var onlyType T

	var typeAndVar T = x

	_ = unused

	var (
		aligned = x
		vars    = y
		here    = y
	)
}
