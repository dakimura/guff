package inline

import "reflect"

func usePtr(v reflect.Value) bool {
	return v.Kind() == reflect.Ptr
}
