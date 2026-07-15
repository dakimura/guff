package fmt

type fmtError string

func (e fmtError) Error() string { return string(e) }

func Errorf(format string, a ...interface{}) error { return fmtError("") }
