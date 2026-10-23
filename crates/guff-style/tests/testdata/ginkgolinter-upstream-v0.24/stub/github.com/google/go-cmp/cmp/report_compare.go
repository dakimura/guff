package cmp

import (
	"reflect"
)

const numContextRecords = 2

type diffMode byte

const (
	diffUnknown   diffMode = 0
	diffIdentical diffMode = ' '
	diffRemoved   diffMode = '-'
	diffInserted  diffMode = '+'
)

type typeMode int

const (
	emitType typeMode = iota
	elideType
	autoType
)

type formatOptions struct {
	DiffMode diffMode
	TypeMode typeMode
	formatValueOptions
}

func (opts formatOptions) WithDiffMode(d diffMode) formatOptions {
	panic("stub")
}
func (opts formatOptions) WithTypeMode(t typeMode) formatOptions {
	panic("stub")
}
func (opts formatOptions) WithVerbosity(level int) formatOptions {
	panic("stub")
}
func (opts formatOptions) verbosity() uint {
	panic("stub")
}

const maxVerbosityPreset = 6

func verbosityPreset(opts formatOptions, i int) formatOptions {
	panic("stub")
}
func (opts formatOptions) FormatDiff(v *valueNode, ptrs *pointerReferences) (out textNode) {
	panic("stub")
}
func (opts formatOptions) formatDiffList(recs []reportRecord, k reflect.Kind, ptrs *pointerReferences) textNode {
	panic("stub")
}
func coalesceAdjacentRecords(name string, recs []reportRecord) (groups []diffStats) {
	panic("stub")
}
