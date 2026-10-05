//+build !other

package foo

func (g *GenericType1[T1]) multImplsMethodGeneric(f1 T1) {
	DoWork()
}

func (g GenericType2[T1, T2]) multImplsMethodGeneric(f1 T1, f2 T2) T2 {
	DoWork()
	return f2
}
