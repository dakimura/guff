package value

import (
	"reflect"
	"unsafe"
)

type Pointer struct {
	p unsafe.Pointer
	t reflect.Type
}

func PointerOf(v reflect.Value) Pointer {
	panic("stub")
}
func (p Pointer) IsNil() bool {
	panic("stub")
}
func (p Pointer) Uintptr() uintptr {
	panic("stub")
}
