package sort

func Slice(slice any, less func(i, j int) bool) {}
func SliceIsSorted(slice any, less func(i, j int) bool) bool { return true }
func SliceStable(slice any, less func(i, j int) bool) {}
