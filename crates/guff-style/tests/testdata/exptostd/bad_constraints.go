package exptostd

import "golang.org/x/exp/constraints"

type orderedSlice[T constraints.Ordered] []T

func minVal[T constraints.Ordered](_ []T) {}

type orderedIface interface {
	constraints.Ordered
}
