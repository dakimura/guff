package localfix

const Preferred = 1

//go:fix inline
const Legacy = Preferred

func use() int {
	return Preferred
}
