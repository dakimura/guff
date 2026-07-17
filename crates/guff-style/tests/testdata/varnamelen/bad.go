package varnamelen

func Variable_Assign() {
	x := 123 // want: too short
	// fill
	// fill
	// fill
	// fill
	// fill
	_ = x
}

func Variable_ValueSpec() {
	var y = 123 // want: too short
	// fill
	// fill
	// fill
	// fill
	// fill
	_ = y
}

func Param(z int) { // want: too short
	// fill
	// fill
	// fill
	// fill
	// fill
	_ = z
}
