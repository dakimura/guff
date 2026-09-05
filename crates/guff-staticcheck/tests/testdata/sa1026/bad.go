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
