package ok

import (
	"b"
	"errors"
	"io"
)

var st = struct {
	ErrSt error
}{}

func ok() {
	st.ErrSt = errors.New("foo")
	_ = b.ErrB
	_ = io.EOF
	b.NotErr = "ok to reassign non-Err"
}
