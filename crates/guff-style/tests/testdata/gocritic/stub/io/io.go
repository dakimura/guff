package io

type Reader interface {
	Read(p []byte) (n int, err error)
}

type Writer interface {
	Write(p []byte) (n int, err error)
}

type StringWriter interface {
	WriteString(s string) (n int, err error)
}

type ReadCloser interface {
	Reader
	Close() error
}

var EOF error

func WriteString(w Writer, s string) (n int, err error) { return 0, nil }
