package md5
type Hash interface{ Write([]byte) (int, error); Sum([]byte) []byte }
func New() Hash { return nil }
func Sum(data []byte) [16]byte { return [16]byte{} }
