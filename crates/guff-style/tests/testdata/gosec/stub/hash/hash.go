package hash
type Hash interface {
	Write([]byte) (int, error)
	Sum([]byte) []byte
}
