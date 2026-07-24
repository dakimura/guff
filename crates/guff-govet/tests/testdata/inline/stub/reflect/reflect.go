package reflect

type Kind int

const (
	Invalid Kind = iota
	Bool
	Int
	Pointer
	//go:fix inline
	Ptr = Pointer
)

type Value struct{}

func (v Value) Kind() Kind { return Invalid }
