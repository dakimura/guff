package main

import "io"

type nop struct{}

func (nop) Read([]byte) (int, error) { return 0, nilErr{} }
func (nop) Close() error             { return nilErr{} }

func fn1() (io.ReadCloser, error) { return nop{}, nilErr{} }

func fn2() {
	rc, err := fn1()
	defer rc.Close()
	if err != nil {
	}
}

type nilErr struct{}

func (nilErr) Error() string { return "" }
