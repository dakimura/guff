package wrapcheck

// Helper that returns an error without wrapping (same package).
func load() error {
	return nil
}

func useInternal() error {
	err := load()
	return err
}
