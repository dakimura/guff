package unsafefuncs

import "unsafe"

func addConst(ptr unsafe.Pointer) unsafe.Pointer {
	return unsafe.Pointer(uintptr(ptr) + 1)
}

func addVar(ptr unsafe.Pointer, n int) unsafe.Pointer {
	return unsafe.Pointer(uintptr(ptr) + uintptr(n))
}

type uP = unsafe.Pointer

func addAlias(ptr uP) uP {
	return uP(uintptr(ptr) + 1)
}

type namedUP unsafe.Pointer

func skipNamed(ptr namedUP) namedUP {
	return namedUP(uintptr(ptr) + 1)
}
