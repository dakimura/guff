package modernize

import "reflect"

type MyStruct struct{ N int }

type Expr interface {
	String() string
}

func typeofVar() reflect.Type {
	var zero MyStruct
	return reflect.TypeOf(zero)
}

func typeofElem() reflect.Type {
	return reflect.TypeOf((*MyStruct)(nil)).Elem()
}

// Interface-typed args are dynamic; must not suggest TypeFor.
func typeofIface(expr Expr) reflect.Type {
	return reflect.TypeOf(expr)
}
