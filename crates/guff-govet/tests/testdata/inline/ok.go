package inline

import "reflect"

func usePointer(v reflect.Value) bool {
	return v.Kind() == reflect.Pointer
}
