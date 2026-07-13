package p

import "unsafe"

func f() {
	var u uintptr
	_ = unsafe.Pointer(u)
}
