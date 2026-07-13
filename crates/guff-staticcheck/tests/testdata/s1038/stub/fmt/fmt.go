package fmt

type fmtError string
func (e fmtError) Error() string { return string(e) }
func Print(a ...interface{}) (int, error) { return 0, fmtError("") }
func Printf(format string, a ...interface{}) (int, error) { return 0, fmtError("") }
func Sprintf(format string, a ...interface{}) string { return "" }
