package value

import (
	"reflect"
)

var anyType = reflect.TypeOf((*interface{})(nil)).Elem()

func TypeString(t reflect.Type, qualified bool) string {
	panic("stub")
}
func appendTypeName(b []byte, t reflect.Type, qualified, elideFunc bool) []byte {
	panic("stub")
}
