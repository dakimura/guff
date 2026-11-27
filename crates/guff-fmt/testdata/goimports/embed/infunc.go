package infunc

func F() {
	//go:embed data.txt
	var s string
	_ = s
}
