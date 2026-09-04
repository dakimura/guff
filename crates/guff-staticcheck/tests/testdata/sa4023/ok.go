package main

type iface interface{ M() }
type concrete struct{}

func (*concrete) M() {}

func returns() error {
	var err error
	return err
}

func get() (iface, bool) {
	var m map[int]iface
	v, ok := m[0]
	return v, ok
}

// Comparing an interface from a call/map lookup to nil is valid even if the
// same variable is later assigned a concrete pointer (go-redis Manager.Listener).
func reuseListener() iface {
	listener, ok := get()
	if !ok || listener == nil {
		newCredListener := &concrete{}
		listener = newCredListener
	}
	return listener
}

func cond() bool { return true }

// Every function below is a shape upstream stays quiet on. The ones that
// assign conditionally are the reason `irutil.Flatten` gives up when a `Phi`'s
// edges disagree: the zero value is still reachable, so the interface can be
// nil. An AST approximation that only asks "was a concrete pointer assigned
// before this line" reports all of them — buildkit's
// `client/client.go:166` was the first of the kind measured.

// buildkit's shape: the concrete value is assigned inside a loop.
func loopAssign() bool {
	var d iface
	for i := 0; i < 3; i++ {
		if cond() {
			d = &concrete{}
		}
	}
	return d != nil
}

// The same, one level shallower.
func ifAssign() bool {
	var d iface
	if cond() {
		d = &concrete{}
	}
	return d != nil
}

// Both branches assign a concrete value, but not the *same* value, so the
// edges still disagree.
func bothBranches() bool {
	var d iface
	if cond() {
		d = &concrete{}
	} else {
		d = &concrete{}
	}
	return d != nil
}

// The concrete assignment only happens after the comparison.
func afterOnly() bool {
	var d iface
	r := d != nil
	d = &concrete{}
	_ = d
	return r
}

// nil on the left. Upstream carries a "TODO support swapped X and Y".
func swapped() bool {
	var d iface = &concrete{}
	return nil != d
}

type holder struct{ f iface }

// A struct field is never lifted into a register.
func field() bool {
	var s holder
	s.f = &concrete{}
	return s.f != nil
}

var global iface

// Nor is a package-level variable.
func globalVar() bool {
	global = &concrete{}
	return global != nil
}

// A local captured by a closure stays in memory.
func captured() bool {
	var d iface = &concrete{}
	f := func() { d = nil }
	f()
	return d != nil
}

// Type parameters whose constraint has no structural terms: the boxed value
// can still be nil, so the comparison is not always true.

func typeParamAny[T any](v T) bool {
	var d any = v
	return d != nil
}

func typeParamComparable[T comparable](v T) bool {
	var d any = v
	return d != nil
}

func typeParamMethodOnly[T iface](v T) bool {
	var d any = v
	return d != nil
}

type box[T any] struct{ v T }

func boxField[T any](b box[T]) bool {
	var d any = b.v
	return d != nil
}

func main() {
	_ = returns() == nil
	_ = reuseListener()
}
