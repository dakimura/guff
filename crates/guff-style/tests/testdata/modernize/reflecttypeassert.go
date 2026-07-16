//go:build go1.25

package reflecttypeassert

import (
	"io"
	"reflect"
)

type payload struct{ n int }

func twoValued(v reflect.Value) {
	x, ok := v.Interface().(string)
	_, _ = x, ok

	p, ok := v.Interface().(payload)
	_, _ = p, ok

	r, ok := v.Interface().(io.Reader)
	_, _ = r, ok
}

func assignment(v reflect.Value) {
	var y int
	var ok bool
	y, ok = v.Interface().(int)
	_, _ = y, ok
}

func inIfInit(v reflect.Value) {
	if s, ok := v.Interface().(string); ok {
		_ = s
	}
}

func receiverExpr(vs []reflect.Value) {
	e, ok := vs[0].Interface().(error)
	_, _ = e, ok
}

func nomatch(v reflect.Value, pv *reflect.Value, any1 any) {
	// Single-valued assertion panics on failure; TypeAssert doesn't.
	s := v.Interface().(string)
	_ = s

	// Not a type assertion on Value.Interface.
	i, ok := any1.(int)
	_, _ = i, ok

	// Type switches have no TypeAssert equivalent.
	switch v.Interface().(type) {
	case string:
	}

	// Pointer receiver would need an explicit dereference; leave it alone.
	ps, ok := pv.Interface().(string)
	_, _ = ps, ok

	// Interface method value invocation, not part of an assignment.
	_ = v.Interface()
}
