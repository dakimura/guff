package reflect

type Type interface {
	Elem() Type
}

func TypeOf(i any) Type { return nil }
func TypeFor[T any]() Type { return nil }
