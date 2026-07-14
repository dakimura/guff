package prealloc

func Copy(source []int) []int {
	var dest []int
	for _, v := range source {
		dest = append(dest, v)
	}
	return dest
}
