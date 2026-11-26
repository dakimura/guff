package prealloc

func Copy(source []int) []int {
	dest := make([]int, 0, len(source))
	for _, v := range source {
		dest = append(dest, v)
	}
	return dest
}

func NoAppend() []int {
	var dest []int
	return dest
}
