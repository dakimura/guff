package p

func Bad(err error) error {
	if err != nil {
		return nil
	}
	return nil
}
