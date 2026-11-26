package p

func Bad() (n int) {
	n = 1
	return n
}

type Command struct{}

// The message prints the *type*, rendered as `go/types.ExprString` does. A
// function type came out as the placeholder `func(...)`, and a directional
// channel lost its arrow — neither is a type anyone wrote. The fixture above
// returns an `int`, so nothing here was reachable from it.
func BadFuncType() (f func(*Command) error) { return nil }

func BadFuncTypeNamed() (g func(a int, b string) (bool, error)) { return nil }

func BadFuncTypeEmpty() (h func()) { return nil }

func BadRecvChan() (c <-chan int) { return nil }

func BadSendChan() (c chan<- int) { return nil }

func BadBiChan() (c chan int) { return nil }

// The position is the `func` keyword, not the named return — the two agree for
// every declaration at the left margin and part company inside a literal.
var BadLit = func() (f func(*Command) error) { return nil }

// The old walker fell back to a placeholder for everything below: a non-empty
// struct or interface, a generic instantiation (an IndexListExpr), and a
// pointer to one. `types.ExprString` renders each of them.

type Pair[K comparable, V any] struct {
	K K
	V V
}

func BadNonEmptyStruct() (s struct{ A int }) { return }

func BadNonEmptyIface() (i interface{ Foo() int }) { return nil }

func BadGeneric() (p Pair[string, int]) { return }

func BadGenericPtr() (p *Pair[string, []int]) { return nil }

func BadNestedMap() (m map[string]func(int) error) { return nil }

func BadArrayOfChan() (a [3]<-chan struct{ B bool }) { return }
