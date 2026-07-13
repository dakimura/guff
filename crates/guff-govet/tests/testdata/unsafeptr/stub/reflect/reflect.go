package reflect

type Value struct{}

func (v Value) Pointer() uintptr {
	return 0
}

type SliceHeader struct {
	Data uintptr
	Len  int
	Cap  int
}

type StringHeader struct {
	Data uintptr
	Len  int
}
