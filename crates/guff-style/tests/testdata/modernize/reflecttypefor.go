package modernize

import "reflect"

type MyStruct struct{ N int }

func typeofVar() reflect.Type {
	var zero MyStruct
	return reflect.TypeOf(zero)
}

func typeofElem() reflect.Type {
	return reflect.TypeOf((*MyStruct)(nil)).Elem()
}
