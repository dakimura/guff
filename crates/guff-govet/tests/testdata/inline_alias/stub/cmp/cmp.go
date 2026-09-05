// Package cmp is the fixture stub for the right-hand side of an inlinable
// type alias. Only `Ordered` is needed.
package cmp

type Ordered interface {
	~int | ~int8 | ~int16 | ~int32 | ~int64 |
		~uint | ~uint8 | ~uint16 | ~uint32 | ~uint64 | ~uintptr |
		~float32 | ~float64 | ~string
}
