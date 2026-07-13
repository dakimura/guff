package io

type Seeker interface {
	Seek(offset int64, whence int) (int64, error)
}

const (
	SeekStart   = 0
	SeekCurrent = 1
	SeekEnd     = 2
)
