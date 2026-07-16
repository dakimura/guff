package p

const ExistingConst = "repeated value"

func okMatch() {
	a := "repeated value"
	b := "repeated value"
	_ = a
	_ = b
	_ = ExistingConst
}
