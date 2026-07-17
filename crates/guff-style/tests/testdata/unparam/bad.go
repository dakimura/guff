package unparam

func example(used int, unused string) {
	_ = used
}

func withBlank(_ int, y int) {
	_ = y
}

func stub(unused int) {
	panic("not implemented")
}

func ExportedUnused(x int) {}
