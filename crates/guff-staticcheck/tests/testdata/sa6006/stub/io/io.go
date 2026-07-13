package io

type Writer interface {
	Write([]byte) (int, error)
}

func WriteString(w Writer, s string) (n int, err error) { return 0, nilErr{} }

type nilErr struct{}

func (nilErr) Error() string { return "" }
