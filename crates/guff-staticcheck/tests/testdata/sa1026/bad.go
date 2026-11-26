package main

import "encoding/json"

type HasChan struct {
	Ch chan int
}

type HasFunc struct {
	Run func()
}

func main() {
	json.Marshal(HasChan{})
	json.Marshal(HasFunc{})
	var ch chan int
	json.Marshal(ch)
}

// `newTypeEncoder` opens with four short-circuits, and a type that marshals
// itself is never walked:
//
//	if t.Implements(Interfaces["encoding/json.Marshaler"]) { return nil }
//	if !t.IsPtr() && t.CanAddr() && PtrTo(t).Implements(…) { return nil }
//	if t.Implements(Interfaces["encoding.TextMarshaler"]) { return nil }
//	if !t.IsPtr() && t.CanAddr() && PtrTo(t).Implements(…) { return nil }
//
// `CanAddr` is false at the top — `fakejson.Marshal` starts from
// `fakereflect.TypeAndCanAddr{Type: v}` — so a pointer-receiver marshaler
// does *not* cover a value passed by value. That is the shape below.

// PtrJSONer marshals itself, but only through `*PtrJSONer`.
type PtrJSONer struct{ C chan int }

func (j *PtrJSONer) MarshalJSON() ([]byte, error) { return nil, nil }

// PlainKey is a struct key with no MarshalText.
type PlainKey struct{ N int }

// PtrTextKey has one, on the pointer — and `newMapEncoder` has no `PtrTo`
// variant, so the key still fails.
type PtrTextKey struct{ N int }

func (k *PtrTextKey) MarshalText() ([]byte, error) { return nil, nil }

type wrapsPlainKey struct {
	M map[PlainKey][]int
}

func reported(a PtrJSONer, b map[PlainKey][]int, c map[PtrTextKey][]int, d wrapsPlainKey) {
	// The value's method set has no MarshalJSON, and CanAddr is false.
	json.Marshal(a)
	json.Marshal(b)
	json.Marshal(c)
	// The type name is written relative to the package under analysis, so
	// this one reads `map[PlainKey][]int, via x.M`.
	json.Marshal(d)
}

// A map key that is a *type parameter* is not checked at all.
//
//	if typeparams.IsTypeParam(t.Key().Type) {
//		// We don't know enough about the concrete instantiation to say much
//		// about the key. […] the key might implement TextMarshaler.
//		return enc.newTypeEncoder(t.Elem(), stack+"[k]")
//	}
//
// guff ran the key check anyway, so `json.Marshal` of a `map[K]V` field was a
// finding — ava-labs/avalanchego's `BiMap.MarshalJSON`.

type BiMap[K comparable, V any] struct{ keyToValue map[K]V }

func (m *BiMap[K, V]) MarshalJSON() ([]byte, error) {
	// silent: K is a type parameter, so the key is not judged; V's underlying
	// type is its constraint's interface, which `newTypeEncoder` accepts.
	return json.Marshal(m.keyToValue)
}

type keyedByParam[K comparable] struct{ m map[K]int }

func (k *keyedByParam[K]) Marshal() ([]byte, error) {
	return json.Marshal(k.m) // silent, same reason
}

func typeParamElem[V any](m map[string]V) ([]byte, error) {
	return json.Marshal(m) // silent: a concrete key, and V is a type parameter
}
