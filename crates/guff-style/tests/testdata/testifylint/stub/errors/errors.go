package errors

func New(text string) error { return nil }
func Is(err, target error) bool { return false }
func As(err error, target interface{}) bool { return false }
