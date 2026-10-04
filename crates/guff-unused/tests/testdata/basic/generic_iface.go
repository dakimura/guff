// staticcheck's unused draws an edge from a concrete method to the interface
// method it implements. Through v0.7.0 (golangci-lint 2.12.2) that missed an
// interface whose methods use its type parameters inside other types (`[]T`):
// dapr's `pkg/runtime/hotreload/loader/operator` silenced forty findings with
// `//nolint:unused`. v0.8.1 (2.14.0) unifies signatures and finds the edge.
// The non-generic pair below is the control: implementing a used interface
// method there *is* a use, and neither tool reports it.
package genericiface

type item interface{ ~int | ~string }

type streamer[T item] interface {
	list() ([]T, error)
	closeIt() error
}

type resource[T item] struct {
	s streamer[T]
}

func (r *resource[T]) Run() error {
	if _, err := r.s.list(); err != nil {
		return err
	}
	return r.s.closeIt()
}

type comps struct{}

func (c *comps) list() ([]int, error) { return nil, nil }
func (c *comps) closeIt() error       { return nil }

func New() *resource[int] {
	return &resource[int]{s: new(comps)}
}

type plainStreamer interface {
	fetch() error
}

type plainRes struct{ s plainStreamer }

func (r *plainRes) Run() error { return r.s.fetch() }

type plainImpl struct{}

func (p *plainImpl) fetch() error { return nil }

func NewPlain() *plainRes { return &plainRes{s: new(plainImpl)} }
