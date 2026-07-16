package bytes

func HasPrefix(s, prefix []byte) bool { return false }
func HasSuffix(s, suffix []byte) bool { return false }
func TrimPrefix(s, prefix []byte) []byte { return s }
func TrimSuffix(s, suffix []byte) []byte { return s }
func CutPrefix(s, prefix []byte) (after []byte, found bool) { return s, false }
func CutSuffix(s, suffix []byte) (before []byte, found bool) { return s, false }
func Cut(s, sep []byte) (before, after []byte, found bool) { return s, nil, false }
func Split(s, sep []byte) [][]byte { return nil }
func SplitN(s, sep []byte, n int) [][]byte { return nil }
