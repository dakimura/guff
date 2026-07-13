package main

type nilErr struct{}

func (nilErr) Error() string { return "" }

type T struct{}

func (T) Write(b []byte) (int, error) {
	_ = b
	return 0, nilErr{}
}
