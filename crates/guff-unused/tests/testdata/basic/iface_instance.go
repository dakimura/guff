// A generic sealing interface: the concrete types are tied to it by nothing but
// a `var _` conversion, and their methods must stay alive anyway.
//
// What decides is `unused`'s own `implements` (unused/implements.go), which is
// *not* `types.Implements`: an interface method's bare `T` matches any type
// satisfying the constraint, while a `T` nested inside another type — dapr's
// `list() ([]T, error)` in generic_iface.go — matches nothing. So the sealing
// methods here are used and dapr's ten streamers are findings, on the same
// rule. opentofu writes this shape six times in
// `internal/engine/internal/execgraph/result.go`.
//
// The two fixtures differ only by signature, never by method name, so a port
// that matches interface methods by name has to get one of them wrong.
package ifaceinstance

type ResultRef[T any] interface {
	sigil(T)
	AnyRef
}

type AnyRef interface {
	anySigil()
}

// silent — `ResultRef[string]` is instantiated right below, and valueRef
// implements *that*.
type valueRef struct{ index int }

var _ ResultRef[string] = valueRef{}

func (v valueRef) sigil(string) {}

func (v valueRef) anySigil() {}

// silent too, and for a reason worth writing down: upstream's checker binds a
// **bare** type parameter to whatever the concrete method uses, so
// `sigil(int)` is compatible with `sigil(T)` even though the source never
// writes `ResultRef[int]`. Only a type parameter *inside* another type —
// dapr's `list() ([]T, error)` — fails to match. Asking `types.Implements`
// here instead would report this method and disagree with upstream.
type otherRef struct{ index int }

func (o otherRef) sigil(int) {}

func (o otherRef) anySigil() {}

func Use() (AnyRef, AnyRef) { return valueRef{}, otherRef{} }
