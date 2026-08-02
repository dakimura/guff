package maps

func Clone[M ~map[K]V, K comparable, V any](m M) M { return m }
