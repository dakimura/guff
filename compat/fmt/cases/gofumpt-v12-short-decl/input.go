package p

var global = x

func f() {
	var local = x
	var local2, local3 = x, y

	var onlyType T

	var typeAndVar T = x

	var _ = unused

	var (
		aligned = x
		vars    = y
		here    = y
	)
}
