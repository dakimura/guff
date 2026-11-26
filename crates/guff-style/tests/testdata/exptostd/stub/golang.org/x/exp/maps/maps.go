package maps

func Clone(m map[string]string) map[string]string { return m }
func Equal(m1, m2 map[string]string) bool          { return true }
func EqualFunc(m1, m2 map[string]string, eq func(string, string) bool) bool {
	return true
}
func Copy(dst, src map[string]string) {}
func DeleteFunc(m map[string]string, del func(string, string) bool) {}
func Clear(m map[string]string) {}
func Keys(m map[string]string) []string   { return nil }
func Values(m map[string]string) []string { return nil }
