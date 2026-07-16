package emptydecl

type S struct {
	A string
}

func f() {
	var x = S{}
	y := S{}
	_ = x
	_ = y
}
