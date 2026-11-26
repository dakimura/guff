package sha256
type Hash interface{ Write([]byte) (int, error); Sum([]byte) []byte }
func New() Hash { return nil }
