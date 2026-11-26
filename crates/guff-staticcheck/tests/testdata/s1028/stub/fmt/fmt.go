package fmt

type fmtError string
func (e fmtError) Error() string { return string(e) }
func Sprintf(format string, a ...interface{}) string { return "" }
func Errorf(format string, a ...interface{}) error { return fmtError("") }
