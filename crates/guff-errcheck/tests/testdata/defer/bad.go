package defercheck

func teardown() error { return nil }

func bad() {
	defer teardown()
}
