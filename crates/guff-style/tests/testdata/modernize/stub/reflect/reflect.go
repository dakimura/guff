package reflect

type Type interface {
	Elem() Type
}

type Value struct{}

func (Value) Interface() any { return nil }

func TypeOf(i any) Type { return nil }
func TypeFor[T any]() Type { return nil }
func TypeAssert[T any](v Value) (T, bool) {
	var zero T
	return zero, false
}
