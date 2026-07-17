package sha1
type Hash interface{ Write([]byte) (int, error); Sum([]byte) []byte }
func New() Hash { return nil }
func Sum(data []byte) [20]byte { return [20]byte{} }
