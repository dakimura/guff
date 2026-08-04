package p

func Bad() {
	var Ä int // non-ASCII
	_ = Ä
}
