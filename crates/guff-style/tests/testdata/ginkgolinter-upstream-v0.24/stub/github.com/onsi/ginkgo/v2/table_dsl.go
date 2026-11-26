package ginkgo

import (
	"context"
	"reflect"
	"github.com/onsi/ginkgo/v2/types"
)

type EntryDescription string

func (ed EntryDescription) render(args ...any) string {
	panic("stub")
}
func DescribeTable(description string, args ...any) bool {
	panic("stub")
}
func FDescribeTable(description string, args ...any) bool {
	panic("stub")
}
func PDescribeTable(description string, args ...any) bool {
	panic("stub")
}

var XDescribeTable = PDescribeTable

func DescribeTableSubtree(description string, args ...any) bool {
	panic("stub")
}
func FDescribeTableSubtree(description string, args ...any) bool {
	panic("stub")
}
func PDescribeTableSubtree(description string, args ...any) bool {
	panic("stub")
}

var XDescribeTableSubtree = PDescribeTableSubtree

type TableEntry struct {
	description  any
	decorations  []any
	parameters   []any
	codeLocation types.CodeLocation
}

func Entry(description any, args ...any) TableEntry {
	panic("stub")
}
func FEntry(description any, args ...any) TableEntry {
	panic("stub")
}
func PEntry(description any, args ...any) TableEntry {
	panic("stub")
}

var XEntry = PEntry
var contextType = reflect.TypeOf(new(context.Context)).Elem()
var specContextType = reflect.TypeOf(new(SpecContext)).Elem()

func generateTable(description string, isSubtree bool, args ...any) {
	panic("stub")
}
func invokeFunction(function any, parameters []any) []reflect.Value {
	panic("stub")
}
func validateParameters(function any, parameters []any, kind string, cl types.CodeLocation, hasContext bool) error {
	panic("stub")
}
func computeValue(parameter any, t reflect.Type) reflect.Value {
	panic("stub")
}
