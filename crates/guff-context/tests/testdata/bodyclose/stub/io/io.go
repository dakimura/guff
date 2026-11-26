package io

func Copy(dst Writer, src Reader) (written int64, err error) {
	return 0, nil
}

func ReadAll(r Reader) ([]byte, error) {
	return nil, nil
}

type Reader interface {
	Read(p []byte) (n int, err error)
}

type Writer interface {
	Write(p []byte) (n int, err error)
}

type Closer interface {
	Close() error
}

type ReadCloser interface {
	Reader
	Closer
}

var Discard Writer
