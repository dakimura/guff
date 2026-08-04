package p

func Bad() {
	i := 0
	i += 1
}

func exportedWithoutDoc() {}

func BadNames() {
	var Id int
	_ = Id
}
