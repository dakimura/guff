package internal

import (
	"github.com/onsi/ginkgo/v2/types"
)

type SortableSpecs struct {
	Specs   Specs
	Indexes []int
}

func NewSortableSpecs(specs Specs) *SortableSpecs {
	panic("stub")
}
func (s *SortableSpecs) Len() int {
	panic("stub")
}
func (s *SortableSpecs) Swap(i, j int) {
	panic("stub")
}
func (s *SortableSpecs) Less(i, j int) bool {
	panic("stub")
}

type GroupedSpecIndices []SpecIndices
type SpecIndices []int

func OrderSpecs(specs Specs, suiteConfig types.SuiteConfig) (GroupedSpecIndices, GroupedSpecIndices) {
	panic("stub")
}
