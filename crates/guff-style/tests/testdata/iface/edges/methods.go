package edges

import "io"

// unusedmethod and unexported on shapes upstream's testdata does not pin.

type base interface {
	Base() // used through an embedding interface
	Unused()
}

type derived interface {
	base
	Derived()
}

func useDerived(d derived) {
	d.Base()
	d.Derived()
}

// A generic interface's methods are reached through an instance.
type getter[T any] interface {
	Get() T
	Put(T)
}

func useGetter(g getter[int]) int {
	return g.Get()
}

func local() {
	// A local interface declaration.
	type localIface interface {
		LocalUsed()
		LocalUnused()
	}
	var l localIface
	l.LocalUsed()
}

type (
	// grouped is documented on its spec.
	grouped interface {
		G() // trailing note
	}

	// other is documented too.
	other interface {
		// O is documented.
		O()
	}
)

var _ grouped
var _ other

type closer interface{ Close() error }

func Exported(c closer, m map[string]closer, ch chan<- closer, a [2]closer, w io.Writer, f func(closer)) {
}

func Results() (closer, *closer, []closer) { return nil, nil, nil }

type Repo struct{}

func (Repo) Use(c closer) {}

func (*Repo) UsePtr(c closer) {}

type Gen[T any, U any] struct{}

func (g Gen[T, U]) Use(c closer) {}

func Constrained[t closer](x t) {}

type lower struct{}

func (lower) Exported(c closer) {}
