package slices

func Equal(a, b []string) bool { return true }
func EqualFunc(a, b []string, eq func(string, string) bool) bool { return true }
func Compare(a, b []string) int { return 0 }
func CompareFunc(a, b []string, cmp func(string, string) int) int { return 0 }
func Index(a []string, v string) int { return -1 }
func IndexFunc(a []string, f func(string) bool) int { return -1 }
func Contains(a []string, v string) bool { return false }
func ContainsFunc(a []string, f func(string) bool) bool { return false }
func Insert(a []string, i int, v ...string) []string { return a }
func Delete(a []string, i, j int) []string { return a }
func DeleteFunc(a []string, del func(string) bool) []string { return a }
func Replace(a []string, i, j int, v ...string) []string { return a }
func Clone(a []string) []string { return a }
func Compact(a []string) []string { return a }
func CompactFunc(a []string, eq func(string, string) bool) []string { return a }
func Grow(a []string, n int) []string { return a }
func Clip(a []string) []string { return a }
func Reverse(a []string) {}
func Sort(a []string) {}
func SortFunc(a []string, cmp func(string, string) int) {}
func SortStableFunc(a []string, cmp func(string, string) int) {}
func IsSorted(a []string) bool { return true }
func IsSortedFunc(a []string, cmp func(string, string) int) bool { return true }
func Min(a []string) string { return "" }
func MinFunc(a []string, cmp func(string, string) int) string { return "" }
func Max(a []string) string { return "" }
func MaxFunc(a []string, cmp func(string, string) int) string { return "" }
func BinarySearch(a []string, target string) (int, bool) { return 0, false }
func BinarySearchFunc(a []string, target string, cmp func(string, string) int) (int, bool) {
	return 0, false
}
