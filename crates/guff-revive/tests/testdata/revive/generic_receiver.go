// Package genericreceiver covers `exported` on methods whose receiver is a
// generic type. The receiver has to be spelled the way upstream's
// `typeparams.ReceiverType` spells it: no `*`, no type arguments.
package genericreceiver

// Box is exported, so its undocumented methods are reported — as `Box.Get`,
// not `*Box.Get` and not a debug dump of the receiver's AST.
type Box[T any] struct{ v T }

func (b *Box[T]) Get() T { return b.v }

// Pair has two type parameters, so the receiver is an IndexListExpr.
type Pair[K comparable, V any] struct {
	k K
	v V
}

func (p *Pair[K, V]) First() K { return p.k }

// hidden is unexported: `exported` skips its methods, and a receiver rendered
// as anything upper-case would turn that skip into a false positive.
type hidden[T any] struct{ v T }

func (h *hidden[T]) Exported() T { return h.v }
