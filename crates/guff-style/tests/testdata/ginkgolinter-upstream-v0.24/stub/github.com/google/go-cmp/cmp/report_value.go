package cmp

import (
	"reflect"
)

type valueNode struct {
	parent          *valueNode
	Type            reflect.Type
	ValueX          reflect.Value
	ValueY          reflect.Value
	NumSame         int
	NumDiff         int
	NumIgnored      int
	NumCompared     int
	NumTransformed  int
	NumChildren     int
	MaxDepth        int
	Records         []reportRecord
	Value           *valueNode
	TransformerName string
}
type reportRecord struct {
	Key   reflect.Value
	Value *valueNode
}

func (parent *valueNode) PushStep(ps PathStep) (child *valueNode) {
	panic("stub")
}
func (r *valueNode) Report(rs Result) {
	panic("stub")
}
func (child *valueNode) PopStep() (parent *valueNode) {
	panic("stub")
}
