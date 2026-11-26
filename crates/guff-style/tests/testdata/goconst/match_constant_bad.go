package p

const ExistingConst = "repeated value"

func badMatch() {
	a := "repeated value"
	b := "repeated value"
	c := "repeated value"
	_ = a
	_ = b
	_ = c
	_ = ExistingConst
}
