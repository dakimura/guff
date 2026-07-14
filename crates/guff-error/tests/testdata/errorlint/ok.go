package errorlint

func ok(err error) {
	if err != nil {
		_ = err
	}
}
