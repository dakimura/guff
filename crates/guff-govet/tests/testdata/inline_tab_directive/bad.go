package inlinetab

import "example.com/govet/inlinetab/dep"

const Preferred = 2

//go:fix	inline
const Legacy = Preferred

type Real int

//go:fix 	 inline
type Alias = Real

func use() (int, int, Alias, dep.A, *int, int) {
	return Legacy, dep.Old, 0, 0, dep.G(1), dep.Glued
}
