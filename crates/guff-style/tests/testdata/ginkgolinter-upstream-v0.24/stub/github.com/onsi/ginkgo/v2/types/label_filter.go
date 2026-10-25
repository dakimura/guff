package types

import (
	"regexp"
)

var DEBUG_LABEL_FILTER_PARSING = false

type LabelFilter func([]string) bool

func matchLabelAction(label string) LabelFilter {
	panic("stub")
}
func matchLabelRegexAction(regex *regexp.Regexp) LabelFilter {
	panic("stub")
}
func notAction(filter LabelFilter) LabelFilter {
	panic("stub")
}
func andAction(a, b LabelFilter) LabelFilter {
	panic("stub")
}
func orAction(a, b LabelFilter) LabelFilter {
	panic("stub")
}
func labelSetFor(key string, labels []string) map[string]bool {
	panic("stub")
}
func isEmptyLabelSetAction(key string) LabelFilter {
	panic("stub")
}
func containsAnyLabelSetAction(key string, expectedValues []string) LabelFilter {
	panic("stub")
}
func containsAllLabelSetAction(key string, expectedValues []string) LabelFilter {
	panic("stub")
}
func consistsOfLabelSetAction(key string, expectedValues []string) LabelFilter {
	panic("stub")
}
func isSubsetOfLabelSetAction(key string, expectedValues []string) LabelFilter {
	panic("stub")
}

type lfToken uint

const (
	lfTokenInvalid lfToken = iota
	lfTokenRoot
	lfTokenOpenGroup
	lfTokenCloseGroup
	lfTokenNot
	lfTokenAnd
	lfTokenOr
	lfTokenRegexp
	lfTokenLabel
	lfTokenSetKey
	lfTokenSetOperation
	lfTokenSetArgument
	lfTokenEOF
)

func (l lfToken) Precedence() int {
	panic("stub")
}
func (l lfToken) String() string {
	panic("stub")
}

type treeNode struct {
	token     lfToken
	location  int
	value     string
	parent    *treeNode
	leftNode  *treeNode
	rightNode *treeNode
}

func (tn *treeNode) setRightNode(node *treeNode) {
	panic("stub")
}
func (tn *treeNode) setLeftNode(node *treeNode) {
	panic("stub")
}
func (tn *treeNode) firstAncestorWithPrecedenceLEQ(precedence int) *treeNode {
	panic("stub")
}
func (tn *treeNode) firstUnmatchedOpenNode() *treeNode {
	panic("stub")
}
func (tn *treeNode) constructLabelFilter(input string) (LabelFilter, error) {
	panic("stub")
}
func (tn *treeNode) tokenString() string {
	panic("stub")
}
func (tn *treeNode) toString(indent int) string {
	panic("stub")
}

var validSetOperations = map[string]string{"containsany": "containsAny", "containsall": "containsAll", "consistsof": "consistsOf", "issubsetof": "isSubsetOf", "isempty": "isEmpty"}

func tokenize(input string) func() (*treeNode, error) {
	panic("stub")
}
func MustParseLabelFilter(input string) LabelFilter {
	panic("stub")
}
func ParseLabelFilter(input string) (LabelFilter, error) {
	panic("stub")
}
func ValidateAndCleanupLabel(label string, cl CodeLocation) (string, error) {
	panic("stub")
}
