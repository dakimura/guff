package bytes

func Replace(s, old, new []byte, n int) []byte { return s }
func SplitN(s, sep []byte, n int) [][]byte     { return nil }
func Contains(b, subslice []byte) bool         { return false }
func Compare(a, b []byte) int                  { return 0 }
func Equal(a, b []byte) bool                   { return false }
func EqualFold(s, t []byte) bool               { return false }
func ToLower(s []byte) []byte                  { return s }
func ToUpper(s []byte) []byte                  { return s }
func HasPrefix(s, prefix []byte) bool          { return false }
func HasSuffix(s, suffix []byte) bool          { return false }
func Index(s, sep []byte) int                  { return -1 }
func Repeat(b []byte, count int) []byte        { return nil }

// dupArg patterns guff did not know.
func LastIndex(s, sep []byte) int               { return -1 }
func Split(s, sep []byte) [][]byte              { return nil }
func SplitAfter(s, sep []byte) [][]byte         { return nil }
func SplitAfterN(s, sep []byte, n int) [][]byte { return nil }
func ReplaceAll(s, old, new []byte) []byte      { return s }
