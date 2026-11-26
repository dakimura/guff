package bad

var b bool

func bad() {
	var x int
	switch b {
	default:
		x = 0
		fallthrough
	case b:
	}
	x = 0
	_ = x
}
