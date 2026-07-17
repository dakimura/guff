package gzip

import "io"

type Reader struct{}

func NewReader(r io.Reader) (*Reader, error) {
	return nil, nil
}

func (r *Reader) Read(p []byte) (int, error) {
	return 0, nil
}
