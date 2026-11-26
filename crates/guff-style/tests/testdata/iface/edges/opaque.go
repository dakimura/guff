package edges

// opaque keys the types it collects by go/types' *identity*: a *Named is
// shared, but every `&x`, `new(T)` and type expression builds a fresh
// *Pointer. Each function below pins one way a pointer type is (or is not)
// the same value on two return paths.

type Server interface{ Serve() error }

type server struct{ a int }

func (s server) Serve() error { return nil }

// Two `&server{}`: two pointers, so two implementations — not reported.
func TwoAddrs(x bool) Server {
	if x {
		return &server{}
	}
	return &server{a: 1}
}

// One variable returned twice: one pointer.
func SameVar(x bool) Server {
	s := &server{}
	if x {
		return s
	}
	return s
}

func mk() *server { return nil }

// One callee: its signature's result, one pointer.
func SameCallee(x bool) Server {
	if x {
		return mk()
	}
	return mk()
}

// `b := a` gives b a's type.
func CopiedVar(x bool) Server {
	a := &server{}
	b := a
	if x {
		return a
	}
	return b
}

// `var a, b *server` evaluates the type once per name.
func OneSpecTwoNames(x bool) Server {
	var a, b *server
	if x {
		return a
	}
	return b
}

// Parameters of one field share the field's type.
func OneFieldTwoParams(x bool, a, b *server) Server {
	if x {
		return a
	}
	return b
}

func TwoSpecs(x bool) Server {
	var a *server
	var b *server
	if x {
		return a
	}
	return b
}

func TwoAssigns(x bool) (s Server) {
	if x {
		s = &server{}
		return
	}
	s = &server{}
	return
}

func OneAssign(x bool) (s Server) {
	s = &server{}
	return
}

func TwoNews(x bool) Server {
	if x {
		return new(server)
	}
	return new(server)
}

// Values of a Named type are always the same type.
func TwoValues(x bool) Server {
	if x {
		return server{}
	}
	return server{a: 2}
}

func mk2() (*server, error) { return nil, nil }

func SameTupleCallee(x bool) (Server, error) {
	if x {
		return mk2()
	}
	return mk2()
}

func AssignFromTuple() (s Server, err error) {
	s, err = mk2()
	return
}

func FromSlice(x bool, ss []*server) Server {
	if x {
		return ss[0]
	}
	return ss[1]
}

func FromRange(ss []*server) Server {
	for _, s := range ss {
		return s
	}
	return ss[0]
}

type holder struct{ s *server }

func FromField(h holder, x bool) Server {
	if x {
		return h.s
	}
	return h.s
}

// An alias of an interface is not a *Named: fromSamePackage says no.
type ServerAlias = Server

func ViaAlias() ServerAlias {
	return &server{}
}

type Fn interface{ Call() }

type fn func()

func (f fn) Call() { f() }

// A Named function type is skipped.
func NewFn() Fn {
	return fn(func() {})
}

type Box[T any] struct{ v T }

func (b *Box[T]) Serve() error { return nil }

func NewBox() Server {
	return &Box[int]{}
}

func Third() (int, string, Server, error) {
	return 0, "", &server{}, nil
}

func Shadowed() (Server, error) {
	{
		s := &server{}
		_ = s
	}
	return nil, nil
}

func Paren() Server {
	return (&server{})
}
