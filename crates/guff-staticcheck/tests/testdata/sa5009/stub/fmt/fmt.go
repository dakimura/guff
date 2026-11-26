package fmt

func Printf(format string, a ...interface{}) (n int, err error) { return 0, nilErr{} }

type nilErr struct{}

func (nilErr) Error() string { return "" }
