package lib

type Box[T any] struct{ v T }

func (b *Box[T]) Close() error { return nil }
func (b Box[T]) String() string { return "" }
