package unconvert

func Bad() {
	var n int
	_ = int(n)

	type ID string
	var id ID
	_ = ID(id)
}

func KeepUntyped() {
	_ = byte(0)
	_ = int(1)
}

func KeepFloatRounding() {
	var f1, f2, f3 float64
	_ = f1 + float64(f2*f3)
}
