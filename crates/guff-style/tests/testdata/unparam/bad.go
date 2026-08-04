package unparam

func example(used int, unused string) int {
	return used + 1
}

func withBlank(_ int, y int) int {
	return y
}

func stub(unused int) {
	panic("not implemented")
}

func discardOnly(unused int) {
	_ = unused
}

func ExportedUnused(x int) {}
