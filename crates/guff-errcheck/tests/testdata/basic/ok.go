package ok

func returnsError() error {
	return nil
}

func ok() {
	if err := returnsError(); err != nil {
		return
	}
}
