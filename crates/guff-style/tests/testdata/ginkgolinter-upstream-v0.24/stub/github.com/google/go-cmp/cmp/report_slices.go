package cmp

import (
	"reflect"
	"github.com/google/go-cmp/cmp/internal/diff"
)

func (opts formatOptions) CanFormatDiffSlice(v *valueNode) bool {
	panic("stub")
}
func (opts formatOptions) FormatDiffSlice(v *valueNode) textNode {
	panic("stub")
}
func formatASCII(s string) string {
	panic("stub")
}
func (opts formatOptions) formatDiffSlice(vx, vy reflect.Value, chunkSize int, name string, makeRec func(reflect.Value, diffMode) textRecord) (list textList) {
	panic("stub")
}
func coalesceAdjacentEdits(name string, es diff.EditScript) (groups []diffStats) {
	panic("stub")
}
func coalesceInterveningIdentical(groups []diffStats, windowSize int) []diffStats {
	panic("stub")
}
func cleanupSurroundingIdentical(groups []diffStats, eq func(i, j int) bool) []diffStats {
	panic("stub")
}
