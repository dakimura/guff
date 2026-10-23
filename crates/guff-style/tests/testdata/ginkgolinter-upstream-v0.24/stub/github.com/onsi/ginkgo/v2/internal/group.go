package internal

import (
	"github.com/onsi/ginkgo/v2/types"
)

type runOncePair struct {
	nodeID      uint
	nodeType    types.NodeType
	containerID uint
}

func (pair runOncePair) isZero() bool {
	panic("stub")
}
func runOncePairForNode(node Node, containerID uint) runOncePair {
	panic("stub")
}

type runOncePairs []runOncePair

func runOncePairsForSpec(spec Spec) runOncePairs {
	panic("stub")
}
func (pairs runOncePairs) runOncePairFor(nodeID uint) runOncePair {
	panic("stub")
}
func (pairs runOncePairs) hasRunOncePair(pair runOncePair) bool {
	panic("stub")
}
func (pairs runOncePairs) withType(nodeTypes types.NodeType) runOncePairs {
	panic("stub")
}

type group struct {
	suite                  *Suite
	specs                  Specs
	runOncePairs           map[uint]runOncePairs
	runOnceTracker         map[runOncePair]types.SpecState
	succeeded              bool
	failedInARunOnceBefore bool
	continueOnFailure      bool
}

func newGroup(suite *Suite) *group {
	panic("stub")
}
func (g *group) initialReportForSpec(spec Spec) types.SpecReport {
	panic("stub")
}
func constructionNodeReportForTreeNode(node *TreeNode) *types.ConstructionNodeReport {
	panic("stub")
}
func addNodeToReportForNode(report *types.ConstructionNodeReport, node *TreeNode) {
	panic("stub")
}
func (g *group) evaluateSkipStatus(spec Spec) (types.SpecState, types.Failure) {
	panic("stub")
}
func (g *group) isLastSpecWithPair(specID uint, pair runOncePair) bool {
	panic("stub")
}
func (g *group) willRunAnotherAttempt(isFinalAttempt bool) bool {
	panic("stub")
}
func (g *group) attemptSpec(isFinalAttempt bool, spec Spec) bool {
	panic("stub")
}
func (g *group) run(specs Specs) {
	panic("stub")
}
