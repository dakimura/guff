package genericmethod

// honnef names a method by its receiver *type*, and `types` prints a generic
// receiver with its type parameter list — `(*holder[T]).run`, not
// `(*holder).run`. Same finding, same line, different text.
type holder[T any] struct{ item T }

func (h *holder[T]) run() T { return h.item }

type pair[K comparable, V any] struct {
	k K
	v V
}

func (p pair[K, V]) key() K { return p.k }

func New[T any](item T) *holder[T] { return &holder[T]{item: item} }

func NewPair[K comparable, V any](k K, v V) pair[K, V] { return pair[K, V]{k: k, v: v} }
