package internal

import (
	"sync"
	"time"
	"github.com/onsi/ginkgo/v2/internal/interrupt_handler"
	"github.com/onsi/ginkgo/v2/internal/parallel_support"
	"github.com/onsi/ginkgo/v2/reporters"
	"github.com/onsi/ginkgo/v2/types"
)

type Phase uint

const (
	PhaseBuildTopLevel Phase = iota
	PhaseBuildTree
	PhaseRun
)
const ProgressReporterDeadline = 5 * time.Second

type Suite struct {
	tree               *TreeNode
	topLevelContainers Nodes
	*ProgressReporterManager
	phase                         Phase
	suiteNodes                    Nodes
	cleanupNodes                  Nodes
	aroundNodes                   types.AroundNodes
	failer                        *Failer
	reporter                      reporters.Reporter
	writer                        WriterInterface
	outputInterceptor             OutputInterceptor
	interruptHandler              interrupt_handler.InterruptHandlerInterface
	config                        types.SuiteConfig
	deadline                      time.Time
	currentConstructionNodeReport *types.ConstructionNodeReport
	skipAll                       bool
	report                        types.Report
	currentSpecReport             types.SpecReport
	currentNode                   Node
	currentNodeStartTime          time.Time
	currentSpecContext            *specContext
	currentByStep                 types.SpecEvent
	timelineOrder                 int
	selectiveLock                 *sync.Mutex
	client                        parallel_support.Client
}

func NewSuite() *Suite {
	panic("stub")
}
func (suite *Suite) Clone() (*Suite, error) {
	panic("stub")
}
func (suite *Suite) BuildTree() error {
	panic("stub")
}
func (suite *Suite) Run(description string, suiteLabels Labels, suiteSemVerConstraints SemVerConstraints, suiteComponentSemVerConstraints ComponentSemVerConstraints, suiteAroundNodes types.AroundNodes, suitePath string, failer *Failer, reporter reporters.Reporter, writer WriterInterface, outputInterceptor OutputInterceptor, interruptHandler interrupt_handler.InterruptHandlerInterface, client parallel_support.Client, progressSignalRegistrar ProgressSignalRegistrar, suiteConfig types.SuiteConfig) (bool, bool) {
	panic("stub")
}
func (suite *Suite) InRunPhase() bool {
	panic("stub")
}
func (suite *Suite) PushNode(node Node) error {
	panic("stub")
}
func (suite *Suite) pushSuiteNode(node Node) error {
	panic("stub")
}
func (suite *Suite) pushCleanupNode(node Node) error {
	panic("stub")
}
func (suite *Suite) generateTimelineLocation() types.TimelineLocation {
	panic("stub")
}
func (suite *Suite) handleSpecEvent(event types.SpecEvent) types.SpecEvent {
	panic("stub")
}
func (suite *Suite) handleSpecEventEnd(eventType types.SpecEventType, startEvent types.SpecEvent) {
	panic("stub")
}
func (suite *Suite) By(text string, callback ...func()) error {
	panic("stub")
}
func (suite *Suite) CurrentConstructionNodeReport() types.ConstructionNodeReport {
	panic("stub")
}
func (suite *Suite) CurrentSpecReport() types.SpecReport {
	panic("stub")
}
func (suite *Suite) GetPreviewReport() types.Report {
	panic("stub")
}
func (suite *Suite) AddReportEntry(entry ReportEntry) error {
	panic("stub")
}
func (suite *Suite) generateProgressReport(fullReport bool) types.ProgressReport {
	panic("stub")
}
func (suite *Suite) handleProgressSignal() {
	panic("stub")
}
func (suite *Suite) emitProgressReport(report types.ProgressReport) {
	panic("stub")
}
func (suite *Suite) isRunningInParallel() bool {
	panic("stub")
}
func (suite *Suite) processCurrentSpecReport() {
	panic("stub")
}
func (suite *Suite) runSpecs(description string, suiteLabels Labels, suiteSemVerConstraints SemVerConstraints, suiteComponentSemVerConstraints ComponentSemVerConstraints, suitePath string, hasProgrammaticFocus bool, specs Specs) bool {
	panic("stub")
}
func (suite *Suite) runBeforeSuite(numSpecsThatWillBeRun int) {
	panic("stub")
}
func (suite *Suite) runAfterSuiteCleanup(numSpecsThatWillBeRun int) {
	panic("stub")
}
func (suite *Suite) reportEach(spec Spec, nodeType types.NodeType) {
	panic("stub")
}
func (suite *Suite) runSuiteNode(node Node) {
	panic("stub")
}
func (suite *Suite) runReportSuiteNodesIfNeedBe(nodeType types.NodeType) {
	panic("stub")
}
func (suite *Suite) runReportSuiteNode(node Node, report types.Report) {
	panic("stub")
}
func (suite *Suite) runNode(node Node, specDeadline time.Time, text string) (types.SpecState, types.Failure) {
	panic("stub")
}
func (suite *Suite) pauseOnFailureIfRequested(node Node) {
	panic("stub")
}
func (suite *Suite) failureForLeafNodeWithMessage(node Node, message string) types.Failure {
	panic("stub")
}
func max(a, b int) int {
	panic("stub")
}
