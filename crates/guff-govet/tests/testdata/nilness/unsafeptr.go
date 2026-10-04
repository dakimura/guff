package nilness

import "unsafe"

// x/tools v0.50 (golangci-lint 2.14.0): go/ssa's `nillable` counts
// unsafe.Pointer, so its nil is a nil constant and the second comparison is
// as decided as it would be for a *T. v0.44 said nothing about either.
func unsafePointerNil(p unsafe.Pointer) int {
	if p == nil {
		if p == nil {
			return 1
		}
	}
	if p != nil {
		return 2
	}
	return 3
}
