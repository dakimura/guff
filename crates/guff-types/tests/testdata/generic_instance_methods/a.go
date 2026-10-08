// Methods selected on instances of generic types. go/types records an
// expanded copy of the method (receiver `W[string]`, Origin the declared
// method): revive prints that copy's FullName, while printf and unusedresult
// go through typeutil.Callee and name the origin (`W[T]`).
package inst

import (
	"fmt"

	"example.com/inst/lib"
)

type W[T any] struct{ v T }

func (W[T]) Errorf(format string, args ...any) error { return fmt.Errorf(format, args...) }
func (*W[T]) Fail() error                             { return nil }
func (W[T]) String() string                           { return "" }
func (w W[T]) Get() T                                 { return w.v }

type Pair[K comparable, V any] struct{}

func (Pair[K, V]) Close() error { return nil }

type Embeds[T any] struct{ W[T] }

func use() {
	var w W[string]
	w.Errorf("%d", "x")
	w.Fail()
	w.String()
	_ = w.Get()
	var p Pair[int, string]
	p.Close()
	var e Embeds[int]
	e.Fail()
	e.Errorf("%s", 1)
	var b lib.Box[int]
	b.Close()
	b.String()
	f := W[bool].Errorf
	f(W[bool]{}, "%d", "y")
}

var _ = use
