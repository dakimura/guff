package strings

func HasPrefix(s, prefix string) bool   { return false }
func HasSuffix(s, suffix string) bool   { return false }
func TrimPrefix(s, prefix string) string { return s }
func TrimSuffix(s, suffix string) string { return s }
func CutPrefix(s, prefix string) (after string, found bool) { return s, false }
func CutSuffix(s, suffix string) (before string, found bool) { return s, false }
func Cut(s, sep string) (before, after string, found bool) { return s, "", false }
func Split(s, sep string) []string      { return nil }
func SplitN(s, sep string, n int) []string { return nil }
func Fields(s string) []string          { return nil }
func SplitSeq(s, sep string) func(func(string) bool) {
	return func(yield func(string) bool) {}
}
func FieldsSeq(s string) func(func(string) bool) {
	return func(yield func(string) bool) {}
}
