package io

type Reader interface {
	Read([]byte) (int, error)
}

type Writer interface {
	Write([]byte) (int, error)
}

func Copy(dst Writer, src Reader) (int64, error) {
	return 0, nil
}

func CopyBuffer(dst Writer, src Reader, buf []byte) (int64, error) {
	return 0, nil
}

func CopyN(dst Writer, src Reader, n int64) (int64, error) {
	return 0, nil
}

func WriteString(w Writer, s string) (int, error) {
	return 0, nil
}
