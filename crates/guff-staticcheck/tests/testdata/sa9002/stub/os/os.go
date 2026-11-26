package os

type FileMode uint32

func Chmod(name string, mode FileMode) error { return nilErr{} }

type nilErr struct{}

func (nilErr) Error() string { return "" }
