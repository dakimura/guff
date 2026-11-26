package io

type Reader interface{ Read([]byte) (int, error) }

type Closer interface{ Close() error }

type ReadCloser interface {
	Reader
	Closer
}
