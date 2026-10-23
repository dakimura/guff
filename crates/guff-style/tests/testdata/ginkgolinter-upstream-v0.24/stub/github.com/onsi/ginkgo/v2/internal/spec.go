package internal

import (
	"time"
	"github.com/onsi/ginkgo/v2/types"
)

type Spec struct {
	Nodes Nodes
	Skip  bool
}

func (s Spec) SubjectID() uint {
	panic("stub")
}
func (s Spec) Text() string {
	panic("stub")
}
func (s Spec) FirstNodeWithType(nodeTypes types.NodeType) Node {
	panic("stub")
}
func (s Spec) FlakeAttempts() int {
	panic("stub")
}
func (s Spec) MustPassRepeatedly() int {
	panic("stub")
}
func (s Spec) SpecTimeout() time.Duration {
	panic("stub")
}

type Specs []Spec

func (s Specs) HasAnySpecsMarkedPending() bool {
	panic("stub")
}
func (s Specs) CountWithoutSkip() int {
	panic("stub")
}
func (s Specs) AtIndices(indices SpecIndices) Specs {
	panic("stub")
}
