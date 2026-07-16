package example

import (
	"b"
	"errors"
	"io"
)

var st = struct {
	ErrSt error
}{}

func bad() {
	b.ErrB = nil
	io.EOF = nil
	st.ErrSt = errors.New("foo")
	b.NotErr = "is error"
}
