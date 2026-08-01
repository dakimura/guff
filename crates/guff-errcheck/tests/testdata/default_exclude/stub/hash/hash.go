package hash

type Hash interface {
	Write([]byte) (int, error)
	Sum([]byte) []byte
	Reset()
	Size() int
	BlockSize() int
}
