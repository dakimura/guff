package main

type msg interface{ M() }

type other interface{ O() }

type impl struct{}

func (impl) M() {}

func f(i interface{}) { _ = i }

// A different interface.
func toOther(x msg) (other, bool) {
	v, ok := x.(other)
	return v, ok
}

// A concrete type: `types.IsInterface(t1)` is false.
func toConcrete(x msg) (impl, bool) {
	v, ok := x.(impl)
	return v, ok
}

// A type switch has no `expr.Type`, and upstream returns early for it.
func typeSwitch(x msg) int {
	switch x.(type) {
	case impl:
		return 1
	}
	return 0
}
