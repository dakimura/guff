package bad

func returnsError() error {
	return nil
}

func bad() {
	returnsError()
}
