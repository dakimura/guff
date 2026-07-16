//go:build go1.24

package stditerators

import "go/types"

// C-style loop over Struct.{NumFields,Field} -> Struct.Fields iteration.
func useStruct(s *types.Struct) {
	for i := 0; i < s.NumFields(); i++ {
		_ = s.Field(i)
	}
}

// range-over-int form: for i := range s.NumFields().
func useStructRange(s *types.Struct) {
	for i := range s.NumFields() {
		_ = s.Field(i)
	}
}

// Tuple.{Len,At} -> Tuple.Variables iteration.
func useTuple(t *types.Tuple) {
	for i := 0; i < t.Len(); i++ {
		_ = t.At(i)
	}
}

// Not modernizable: the index is used for something other than s.Field(i).
func extraUse(s *types.Struct) int {
	sum := 0
	for i := 0; i < s.NumFields(); i++ {
		_ = s.Field(i)
		sum += i
	}
	return sum
}

// Not modernizable: no matching table type.
func plainSlice(xs []int) {
	for i := 0; i < len(xs); i++ {
		_ = xs[i]
	}
}
