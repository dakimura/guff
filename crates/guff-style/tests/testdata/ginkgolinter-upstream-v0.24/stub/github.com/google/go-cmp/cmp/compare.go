package cmp

import (
	"reflect"
	"github.com/google/go-cmp/cmp/internal/diff"
)

func Equal(x, y interface{}, opts ...Option) bool {
	panic("stub")
}
func Diff(x, y interface{}, opts ...Option) string {
	panic("stub")
}
func rootStep(x, y interface{}) PathStep {
	panic("stub")
}

type state struct {
	result     diff.Result
	curPath    Path
	curPtrs    pointerPath
	reporters  []reporter
	recChecker recChecker
	dynChecker dynChecker
	exporters  []exporter
	opts       Options
}

func newState(opts []Option) *state {
	panic("stub")
}
func (s *state) processOption(opt Option) {
	panic("stub")
}
func (s *state) statelessCompare(step PathStep) diff.Result {
	panic("stub")
}
func (s *state) compareAny(step PathStep) {
	panic("stub")
}
func (s *state) tryOptions(t reflect.Type, vx, vy reflect.Value) bool {
	panic("stub")
}
func (s *state) tryMethod(t reflect.Type, vx, vy reflect.Value) bool {
	panic("stub")
}
func (s *state) callTRFunc(f, v reflect.Value, step Transform) reflect.Value {
	panic("stub")
}
func (s *state) callTTBFunc(f, x, y reflect.Value) bool {
	panic("stub")
}
func detectRaces(c chan<- reflect.Value, f reflect.Value, vs ...reflect.Value) {
	panic("stub")
}
func (s *state) compareStruct(t reflect.Type, vx, vy reflect.Value) {
	panic("stub")
}
func (s *state) compareSlice(t reflect.Type, vx, vy reflect.Value) {
	panic("stub")
}
func (s *state) compareMap(t reflect.Type, vx, vy reflect.Value) {
	panic("stub")
}
func (s *state) comparePtr(t reflect.Type, vx, vy reflect.Value) {
	panic("stub")
}
func (s *state) compareInterface(t reflect.Type, vx, vy reflect.Value) {
	panic("stub")
}
func (s *state) report(eq bool, rf resultFlags) {
	panic("stub")
}

type recChecker struct{ next int }

func (rc *recChecker) Check(p Path) {
	panic("stub")
}

type dynChecker struct{ curr, next int }

func (dc *dynChecker) Next() bool {
	panic("stub")
}
func makeAddressable(v reflect.Value) reflect.Value {
	panic("stub")
}
