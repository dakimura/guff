package strings

func Contains(s, substr string) bool { return false }
func Replace(s, old, new string, n int) string { return s }
func Compare(a, b string) int { return 0 }
func EqualFold(s, t string) bool { return false }
func HasPrefix(s, prefix string) bool { return false }
func HasSuffix(s, suffix string) bool { return false }
func Index(s, substr string) int { return -1 }
func SplitN(s, sep string, n int) []string { return nil }
func Split(s, sep string) []string { return nil }
func Join(elems []string, sep string) string { return "" }
func ToLower(s string) string                { return s }
func ToUpper(s string) string                { return s }
func TrimSpace(s string) string              { return s }

// dupArg patterns guff did not know. `Replace` is already above; note that its
// pattern is `strings.Replace($_, $x, $x, $_)` — arguments 1 and 2.
func LastIndex(s, substr string) int            { return -1 }
func SplitAfter(s, sep string) []string         { return nil }
func SplitAfterN(s, sep string, n int) []string { return nil }
func ReplaceAll(s, old, new string) string      { return s }
