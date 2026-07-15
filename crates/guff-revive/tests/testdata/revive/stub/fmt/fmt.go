package fmt

type fmtError string

func (e fmtError) Error() string { return string(e) }

func Errorf(format string, a ...interface{}) error { return fmtError("") }

func Sprintf(format string, a ...interface{}) string { return "" }

func Printf(format string, a ...interface{}) (int, error) { return 0, nil }

func Print(a ...interface{}) (int, error) { return 0, nil }
