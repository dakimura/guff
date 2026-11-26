package cmp

import (
	"reflect"
)

var (
	anyType    = reflect.TypeOf((*interface{})(nil)).Elem()
	stringType = reflect.TypeOf((*string)(nil)).Elem()
	bytesType  = reflect.TypeOf((*[]byte)(nil)).Elem()
	byteType   = reflect.TypeOf((*byte)(nil)).Elem()
)

type formatValueOptions struct {
	AvoidStringer  bool
	PrintAddresses bool
	QualifiedNames bool
	VerbosityLevel int
	LimitVerbosity bool
}

func (opts formatOptions) FormatType(t reflect.Type, s textNode) textNode {
	panic("stub")
}
func wrapParens(s textNode) textNode {
	panic("stub")
}
func (opts formatOptions) FormatValue(v reflect.Value, parentKind reflect.Kind, ptrs *pointerReferences) (out textNode) {
	panic("stub")
}
func (opts formatOptions) formatString(prefix, s string) textNode {
	panic("stub")
}
func formatMapKey(v reflect.Value, disambiguate bool, ptrs *pointerReferences) string {
	panic("stub")
}
func formatString(s string) string {
	panic("stub")
}
func formatHex(u uint64) string {
	panic("stub")
}
