package cmp

import (
	"reflect"
	"github.com/google/go-cmp/cmp/internal/value"
)

const (
	pointerDelimPrefix = "⟪"
	pointerDelimSuffix = "⟫"
)

func formatPointer(p value.Pointer, withDelims bool) string {
	panic("stub")
}

type pointerReferences [][2]value.Pointer

func (ps *pointerReferences) PushPair(vx, vy reflect.Value, d diffMode, deref bool) (pp [2]value.Pointer) {
	panic("stub")
}
func (ps *pointerReferences) Push(v reflect.Value) (p value.Pointer, seen bool) {
	panic("stub")
}
func (ps *pointerReferences) Pop() {
	panic("stub")
}

type trunkReferences struct{ pp [2]value.Pointer }
type trunkReference struct{ p value.Pointer }
type leafReference struct{ p value.Pointer }

func wrapTrunkReferences(pp [2]value.Pointer, s textNode) textNode {
	panic("stub")
}
func wrapTrunkReference(p value.Pointer, printAddress bool, s textNode) textNode {
	panic("stub")
}
func makeLeafReference(p value.Pointer, printAddress bool) textNode {
	panic("stub")
}
func resolveReferences(s textNode) {
	panic("stub")
}
func formatReference(id uint) string {
	panic("stub")
}
func updateReferencePrefix(prefix, ref string) string {
	panic("stub")
}
