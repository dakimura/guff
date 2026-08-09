package fmt

func Printf(format string, a ...interface{}) (n int, err error) { return 0, nilErr{} }

func Sprintf(format string, a ...interface{}) string { return "" }

type nilErr struct{}

func (nilErr) Error() string { return "" }
