package unparam

func allUsed(x int, y string) {
	_ = x
	println(y)
}

func explicitKeep(unused int) {
	_ = unused
}

func emptyBody(unused int) {}

func onlyReturn(unused int) {
	return
}
