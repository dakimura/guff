package internal

import (
	"context"
	"reflect"
	"sync"
	"time"
	"github.com/onsi/ginkgo/v2/types"
)

var _global_node_id_counter = uint(0)
var _global_id_mutex = &sync.Mutex{}

func UniqueNodeID() uint {
	panic("stub")
}

type Node struct {
	ID                                            uint
	NodeType                                      types.NodeType
	Text                                          string
	Body                                          func(SpecContext)
	CodeLocation                                  types.CodeLocation
	NestingLevel                                  int
	HasContext                                    bool
	SynchronizedBeforeSuiteProc1Body              func(SpecContext) []byte
	SynchronizedBeforeSuiteProc1BodyHasContext    bool
	SynchronizedBeforeSuiteAllProcsBody           func(SpecContext, []byte)
	SynchronizedBeforeSuiteAllProcsBodyHasContext bool
	SynchronizedAfterSuiteAllProcsBody            func(SpecContext)
	SynchronizedAfterSuiteAllProcsBodyHasContext  bool
	SynchronizedAfterSuiteProc1Body               func(SpecContext)
	SynchronizedAfterSuiteProc1BodyHasContext     bool
	ReportEachBody                                func(SpecContext, types.SpecReport)
	ReportSuiteBody                               func(SpecContext, types.Report)
	MarkedFocus                                   bool
	MarkedPending                                 bool
	MarkedSerial                                  bool
	MarkedOrdered                                 bool
	MarkedContinueOnFailure                       bool
	MarkedOncePerOrdered                          bool
	FlakeAttempts                                 int
	MustPassRepeatedly                            int
	Labels                                        Labels
	SemVerConstraints                             SemVerConstraints
	ComponentSemVerConstraints                    ComponentSemVerConstraints
	PollProgressAfter                             time.Duration
	PollProgressInterval                          time.Duration
	NodeTimeout                                   time.Duration
	SpecTimeout                                   time.Duration
	GracePeriod                                   time.Duration
	AroundNodes                                   types.AroundNodes
	HasExplicitlySetSpecPriority                  bool
	SpecPriority                                  int
	NodeIDWhereCleanupWasGenerated                uint
}
type focusType bool
type pendingType bool
type serialType bool
type orderedType bool
type continueOnFailureType bool
type honorsOrderedType bool
type suppressProgressReporting bool

const Focus = focusType(true)
const Pending = pendingType(true)
const Serial = serialType(true)
const Ordered = orderedType(true)
const ContinueOnFailure = continueOnFailureType(true)
const OncePerOrdered = honorsOrderedType(true)
const SuppressProgressReporting = suppressProgressReporting(true)

type FlakeAttempts uint
type MustPassRepeatedly uint
type Offset uint
type Done chan<- any
type PollProgressInterval time.Duration
type PollProgressAfter time.Duration
type NodeTimeout time.Duration
type SpecTimeout time.Duration
type GracePeriod time.Duration
type SpecPriority int
type Labels []string

func (l Labels) MatchesLabelFilter(query string) bool {
	panic("stub")
}

type SemVerConstraints []string

func (svc SemVerConstraints) MatchesSemVerFilter(version string) bool {
	panic("stub")
}

type ComponentSemVerConstraints map[string][]string

func (csvc ComponentSemVerConstraints) MatchesSemVerFilter(component, version string) bool {
	panic("stub")
}
func unionOf[S ~[]E, E comparable](slices ...S) S {
	panic("stub")
}
func UnionOfLabels(labels ...Labels) Labels {
	panic("stub")
}
func UnionOfSemVerConstraints(semVerConstraints ...SemVerConstraints) SemVerConstraints {
	panic("stub")
}
func UnionOfComponentSemVerConstraints(componentSemVerConstraintsSlice ...ComponentSemVerConstraints) ComponentSemVerConstraints {
	panic("stub")
}
func PartitionDecorations(args ...any) ([]any, []any) {
	panic("stub")
}
func isDecoration(arg any) bool {
	panic("stub")
}
func isSliceOfDecorations(slice any) bool {
	panic("stub")
}

var contextType = reflect.TypeOf(new(context.Context)).Elem()
var specContextType = reflect.TypeOf(new(SpecContext)).Elem()

func NewNode(deprecationTracker *types.DeprecationTracker, nodeType types.NodeType, text string, args ...any) (Node, []error) {
	panic("stub")
}

var doneType = reflect.TypeOf(make(Done))

func extractBodyFunction(deprecationTracker *types.DeprecationTracker, cl types.CodeLocation, arg any) (func(SpecContext), bool) {
	panic("stub")
}

var byteType = reflect.TypeOf([]byte{})

func extractSynchronizedBeforeSuiteProc1Body(arg any) (func(SpecContext) []byte, bool) {
	panic("stub")
}
func extractSynchronizedBeforeSuiteAllProcsBody(arg any) (func(SpecContext, []byte), bool) {
	panic("stub")
}

var errInterface = reflect.TypeOf((*error)(nil)).Elem()

func NewCleanupNode(deprecationTracker *types.DeprecationTracker, fail func(string, types.CodeLocation), args ...any) (Node, []error) {
	panic("stub")
}
func (n Node) IsZero() bool {
	panic("stub")
}

type Nodes []Node

func (n Nodes) Clone() Nodes {
	panic("stub")
}
func (n Nodes) CopyAppend(nodes ...Node) Nodes {
	panic("stub")
}
func (n Nodes) SplitAround(pivot Node) (Nodes, Nodes) {
	panic("stub")
}
func (n Nodes) FirstNodeWithType(nodeTypes types.NodeType) Node {
	panic("stub")
}
func (n Nodes) WithType(nodeTypes types.NodeType) Nodes {
	panic("stub")
}
func (n Nodes) WithoutType(nodeTypes types.NodeType) Nodes {
	panic("stub")
}
func (n Nodes) WithoutNode(nodeToExclude Node) Nodes {
	panic("stub")
}
func (n Nodes) Filter(filter func(Node) bool) Nodes {
	panic("stub")
}
func (n Nodes) FirstSatisfying(filter func(Node) bool) Node {
	panic("stub")
}
func (n Nodes) WithinNestingLevel(deepestNestingLevel int) Nodes {
	panic("stub")
}
func (n Nodes) SortedByDescendingNestingLevel() Nodes {
	panic("stub")
}
func (n Nodes) SortedByAscendingNestingLevel() Nodes {
	panic("stub")
}
func (n Nodes) FirstWithNestingLevel(level int) Node {
	panic("stub")
}
func (n Nodes) Reverse() Nodes {
	panic("stub")
}
func (n Nodes) Texts() []string {
	panic("stub")
}
func (n Nodes) Labels() [][]string {
	panic("stub")
}
func (n Nodes) UnionOfLabels() []string {
	panic("stub")
}
func (n Nodes) SemVerConstraints() [][]string {
	panic("stub")
}
func (n Nodes) UnionOfSemVerConstraints() []string {
	panic("stub")
}
func (n Nodes) ComponentSemVerConstraints() []map[string][]string {
	panic("stub")
}
func (n Nodes) UnionOfComponentSemVerConstraints() map[string][]string {
	panic("stub")
}
func (n Nodes) CodeLocations() []types.CodeLocation {
	panic("stub")
}
func (n Nodes) BestTextFor(node Node) string {
	panic("stub")
}
func (n Nodes) ContainsNodeID(id uint) bool {
	panic("stub")
}
func (n Nodes) HasNodeMarkedPending() bool {
	panic("stub")
}
func (n Nodes) HasNodeMarkedFocus() bool {
	panic("stub")
}
func (n Nodes) HasNodeMarkedSerial() bool {
	panic("stub")
}
func (n Nodes) FirstNodeMarkedOrdered() Node {
	panic("stub")
}
func (n Nodes) IndexOfFirstNodeMarkedOrdered() int {
	panic("stub")
}
func (n Nodes) GetMaxFlakeAttempts() int {
	panic("stub")
}
func (n Nodes) GetMaxMustPassRepeatedly() int {
	panic("stub")
}
func (n Nodes) GetSpecPriority() int {
	panic("stub")
}
func UnrollInterfaceSlice(args any) []any {
	panic("stub")
}

type NodeArgsTransformer func(nodeType types.NodeType, offset Offset, text string, args []any) (string, []any, []error)

func AddTreeConstructionNodeArgsTransformer(transformer NodeArgsTransformer) func() {
	panic("stub")
}

var (
	nodeArgsTransformerCounter int64
	nodeArgsTransformers       []registeredNodeArgsTransformer
)

type registeredNodeArgsTransformer struct {
	id          int64
	transformer NodeArgsTransformer
}

func TransformNewNodeArgs(exitIfErrors func([]error), deprecationTracker *types.DeprecationTracker, nodeType types.NodeType, text string, args ...any) (*types.DeprecationTracker, types.NodeType, string, []any) {
	panic("stub")
}
