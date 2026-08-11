package generic

func one[T any]() error { return nil }

func two[A any, B any](a A) error { return nil }

func bad() {
	one[int]()
	two[int, string](1)
}
