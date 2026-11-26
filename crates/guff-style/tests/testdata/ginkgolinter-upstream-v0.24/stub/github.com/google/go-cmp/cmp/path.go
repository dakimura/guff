package cmp

import (
	"reflect"
	"github.com/google/go-cmp/cmp/internal/value"
)

type Path []PathStep
type PathStep interface {
	String() string
	Type() reflect.Type
	Values() (vx, vy reflect.Value)
}

var (
	_ PathStep = StructField{}
	_ PathStep = SliceIndex{}
	_ PathStep = MapIndex{}
	_ PathStep = Indirect{}
	_ PathStep = TypeAssertion{}
	_ PathStep = Transform{}
)

func (pa *Path) push(s PathStep) {
	panic("stub")
}
func (pa *Path) pop() {
	panic("stub")
}
func (pa Path) Last() PathStep {
	panic("stub")
}
func (pa Path) Index(i int) PathStep {
	panic("stub")
}
func (pa Path) String() string {
	panic("stub")
}
func (pa Path) GoString() string {
	panic("stub")
}

type pathStep struct {
	typ    reflect.Type
	vx, vy reflect.Value
}

func (ps pathStep) Type() reflect.Type {
	panic("stub")
}
func (ps pathStep) Values() (vx, vy reflect.Value) {
	panic("stub")
}
func (ps pathStep) String() string {
	panic("stub")
}

type StructField struct{ *structField }
type structField struct {
	pathStep
	name       string
	idx        int
	unexported bool
	mayForce   bool
	paddr      bool
	pvx, pvy   reflect.Value
	field      reflect.StructField
}

func (sf StructField) Type() reflect.Type {
	panic("stub")
}
func (sf StructField) Values() (vx, vy reflect.Value) {
	panic("stub")
}
func (sf StructField) String() string {
	panic("stub")
}
func (sf StructField) Name() string {
	panic("stub")
}
func (sf StructField) Index() int {
	panic("stub")
}

type SliceIndex struct{ *sliceIndex }
type sliceIndex struct {
	pathStep
	xkey, ykey int
	isSlice    bool
}

func (si SliceIndex) Type() reflect.Type {
	panic("stub")
}
func (si SliceIndex) Values() (vx, vy reflect.Value) {
	panic("stub")
}
func (si SliceIndex) String() string {
	panic("stub")
}
func (si SliceIndex) Key() int {
	panic("stub")
}
func (si SliceIndex) SplitKeys() (ix, iy int) {
	panic("stub")
}

type MapIndex struct{ *mapIndex }
type mapIndex struct {
	pathStep
	key reflect.Value
}

func (mi MapIndex) Type() reflect.Type {
	panic("stub")
}
func (mi MapIndex) Values() (vx, vy reflect.Value) {
	panic("stub")
}
func (mi MapIndex) String() string {
	panic("stub")
}
func (mi MapIndex) Key() reflect.Value {
	panic("stub")
}

type Indirect struct{ *indirect }
type indirect struct{ pathStep }

func (in Indirect) Type() reflect.Type {
	panic("stub")
}
func (in Indirect) Values() (vx, vy reflect.Value) {
	panic("stub")
}
func (in Indirect) String() string {
	panic("stub")
}

type TypeAssertion struct{ *typeAssertion }
type typeAssertion struct{ pathStep }

func (ta TypeAssertion) Type() reflect.Type {
	panic("stub")
}
func (ta TypeAssertion) Values() (vx, vy reflect.Value) {
	panic("stub")
}
func (ta TypeAssertion) String() string {
	panic("stub")
}

type Transform struct{ *transform }
type transform struct {
	pathStep
	trans *transformer
}

func (tf Transform) Type() reflect.Type {
	panic("stub")
}
func (tf Transform) Values() (vx, vy reflect.Value) {
	panic("stub")
}
func (tf Transform) String() string {
	panic("stub")
}
func (tf Transform) Name() string {
	panic("stub")
}
func (tf Transform) Func() reflect.Value {
	panic("stub")
}
func (tf Transform) Option() Option {
	panic("stub")
}

type pointerPath struct {
	mx map[value.Pointer]value.Pointer
	my map[value.Pointer]value.Pointer
}

func (p *pointerPath) Init() {
	panic("stub")
}
func (p pointerPath) Push(vx, vy reflect.Value) (equal, visited bool) {
	panic("stub")
}
func (p pointerPath) Pop(vx, vy reflect.Value) {
	panic("stub")
}
func isExported(id string) bool {
	panic("stub")
}
