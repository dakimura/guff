package unconvert

func Ok() {
	var n int
	var f float64
	_ = float64(n)
	_ = int(f)

	type ID string
	var s string
	_ = ID(s)
	_ = string(ID("x"))
}
