// Package fieldsunsafe holds the two struct-field rules that need imports.
//
//	(5.2) converting to or from unsafe.Pointer uses every field
//	(6.6) a struct with a structs.HostLayout field uses every field
//
// The unit-test type-checker has no importer, so only the golden case
// materialises this package — there it is compared against golangci-lint like
// any other, which is the gate that matters for these two.
package fieldsunsafe

import (
	"structs"
	"sync"
	"unsafe"
)

// (5.2)
type unsafeConv struct {
	a int
	b int
}

func UseUnsafeConv(p unsafe.Pointer) *unsafeConv { return (*unsafeConv)(p) }

// (6.6)
type hostLayout struct {
	_     structs.HostLayout
	deadH int
}

func UseHostLayout() hostLayout { return hostLayout{} }

// (6.4) through the standard library: sync.Mutex has exported methods.
type withMutex struct {
	sync.Mutex
	deadM int
}

func UseWithMutex() *withMutex { return &withMutex{} }
