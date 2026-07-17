package io

type Writer interface {
	Write(p []byte) (n int, err error)
}

type StringWriter interface {
	WriteString(s string) (n int, err error)
}
