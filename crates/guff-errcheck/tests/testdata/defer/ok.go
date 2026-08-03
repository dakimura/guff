package defercheck

func teardown() error { return nil }

func ok() {
	defer func() { _ = teardown() }()
}
