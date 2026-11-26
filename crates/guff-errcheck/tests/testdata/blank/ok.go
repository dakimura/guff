package blank

func a() error {
	return nil
}

func b() (string, error) {
	return "", nil
}

func ok() {
	if err := a(); err != nil {
		return
	}
	r, err := b()
	_ = r
	_ = err
}
