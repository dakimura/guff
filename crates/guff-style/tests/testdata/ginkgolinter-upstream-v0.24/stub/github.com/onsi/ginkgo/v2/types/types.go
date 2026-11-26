package types

import (
	"time"
)

const GINKGO_FOCUS_EXIT_CODE = 197

var GINKGO_TIME_FORMAT = "01/02/06 15:04:05.999"

func init() {
	panic("stub")
}

type ConstructionNodeReport struct {
	ContainerHierarchyTexts                      []string
	ContainerHierarchyLocations                  []CodeLocation
	ContainerHierarchyLabels                     [][]string
	ContainerHierarchySemVerConstraints          [][]string
	ContainerHierarchyComponentSemVerConstraints []map[string][]string
	IsSerial                                     bool
	IsInOrderedContainer                         bool
}

func (report ConstructionNodeReport) FullText() string {
	panic("stub")
}
func (report ConstructionNodeReport) Labels() []string {
	panic("stub")
}

type Report struct {
	SuitePath                       string
	SuiteDescription                string
	SuiteLabels                     []string
	SuiteSemVerConstraints          []string
	SuiteComponentSemVerConstraints map[string][]string
	SuiteSucceeded                  bool
	SuiteHasProgrammaticFocus       bool
	SpecialSuiteFailureReasons      []string
	PreRunStats                     PreRunStats
	StartTime                       time.Time
	EndTime                         time.Time
	RunTime                         time.Duration
	SuiteConfig                     SuiteConfig
	SpecReports                     SpecReports
}
type PreRunStats struct {
	TotalSpecs       int
	SpecsThatWillRun int
}

func (report Report) Add(other Report) Report {
	panic("stub")
}

type SpecReport struct {
	ContainerHierarchyTexts                      []string
	ContainerHierarchyLocations                  []CodeLocation
	ContainerHierarchyLabels                     [][]string
	ContainerHierarchySemVerConstraints          [][]string
	ContainerHierarchyComponentSemVerConstraints []map[string][]string
	LeafNodeType                                 NodeType
	LeafNodeLocation                             CodeLocation
	LeafNodeLabels                               []string
	LeafNodeSemVerConstraints                    []string
	LeafNodeComponentSemVerConstraints           map[string][]string
	LeafNodeText                                 string
	SpecPriority                                 int
	State                                        SpecState
	IsSerial                                     bool
	IsInOrderedContainer                         bool
	StartTime                                    time.Time
	EndTime                                      time.Time
	RunTime                                      time.Duration
	ParallelProcess                              int
	RunningInParallel                            bool
	Failure                                      Failure
	NumAttempts                                  int
	MaxFlakeAttempts                             int
	MaxMustPassRepeatedly                        int
	CapturedGinkgoWriterOutput                   string
	CapturedStdOutErr                            string
	ReportEntries                                ReportEntries
	ProgressReports                              []ProgressReport
	AdditionalFailures                           []AdditionalFailure
	SpecEvents                                   SpecEvents
}

func (report SpecReport) MarshalJSON() ([]byte, error) {
	panic("stub")
}
func (report SpecReport) CombinedOutput() string {
	panic("stub")
}
func (report SpecReport) Failed() bool {
	panic("stub")
}
func (report SpecReport) FullText() string {
	panic("stub")
}
func (report SpecReport) Labels() []string {
	panic("stub")
}
func (report SpecReport) SemVerConstraints() []string {
	panic("stub")
}
func (report SpecReport) ComponentSemVerConstraints() map[string][]string {
	panic("stub")
}
func (report SpecReport) MatchesLabelFilter(query string) (bool, error) {
	panic("stub")
}
func (report SpecReport) MatchesSemVerFilter(version string) (bool, error) {
	panic("stub")
}
func (report SpecReport) FileName() string {
	panic("stub")
}
func (report SpecReport) LineNumber() int {
	panic("stub")
}
func (report SpecReport) FailureMessage() string {
	panic("stub")
}
func (report SpecReport) FailureLocation() CodeLocation {
	panic("stub")
}
func (report SpecReport) Timeline() Timeline {
	panic("stub")
}

type SpecReports []SpecReport

func (reports SpecReports) WithLeafNodeType(nodeTypes NodeType) SpecReports {
	panic("stub")
}
func (reports SpecReports) WithState(states SpecState) SpecReports {
	panic("stub")
}
func (reports SpecReports) CountWithState(states SpecState) int {
	panic("stub")
}
func (reports SpecReports) CountOfFlakedSpecs() int {
	panic("stub")
}
func (reports SpecReports) CountOfRepeatedSpecs() int {
	panic("stub")
}

type TimelineLocation struct {
	Offset int `json:",omitempty"`
	Order  int `json:",omitempty"`
	Time   time.Time
}
type TimelineEvent interface{ GetTimelineLocation() TimelineLocation }
type Timeline []TimelineEvent

func (t Timeline) Len() int {
	panic("stub")
}
func (t Timeline) Less(i, j int) bool {
	panic("stub")
}
func (t Timeline) Swap(i, j int) {
	panic("stub")
}
func (t Timeline) WithoutHiddenReportEntries() Timeline {
	panic("stub")
}
func (t Timeline) WithoutVeryVerboseSpecEvents() Timeline {
	panic("stub")
}

type Failure struct {
	Message                   string
	Location                  CodeLocation
	TimelineLocation          TimelineLocation
	ForwardedPanic            string             `json:",omitempty"`
	FailureNodeContext        FailureNodeContext `json:",omitempty"`
	FailureNodeType           NodeType           `json:",omitempty"`
	FailureNodeLocation       CodeLocation       `json:",omitempty"`
	FailureNodeContainerIndex int                `json:",omitempty"`
	ProgressReport            ProgressReport     `json:",omitempty"`
	AdditionalFailure         *AdditionalFailure `json:",omitempty"`
}

func (f Failure) IsZero() bool {
	panic("stub")
}
func (f Failure) GetTimelineLocation() TimelineLocation {
	panic("stub")
}

type FailureNodeContext uint

const (
	FailureNodeContextInvalid FailureNodeContext = iota
	FailureNodeIsLeafNode
	FailureNodeAtTopLevel
	FailureNodeInContainer
)

var fncEnumSupport = NewEnumSupport(map[uint]string{uint(FailureNodeContextInvalid): "INVALID FAILURE NODE CONTEXT", uint(FailureNodeIsLeafNode): "leaf-node", uint(FailureNodeAtTopLevel): "top-level", uint(FailureNodeInContainer): "in-container"})

func (fnc FailureNodeContext) String() string {
	panic("stub")
}
func (fnc *FailureNodeContext) UnmarshalJSON(b []byte) error {
	panic("stub")
}
func (fnc FailureNodeContext) MarshalJSON() ([]byte, error) {
	panic("stub")
}

type AdditionalFailure struct {
	State   SpecState
	Failure Failure
}

func (f AdditionalFailure) GetTimelineLocation() TimelineLocation {
	panic("stub")
}

type SpecState uint

const (
	SpecStateInvalid SpecState = 0
	SpecStatePending SpecState = 1 << iota
	SpecStateSkipped
	SpecStatePassed
	SpecStateFailed
	SpecStateAborted
	SpecStatePanicked
	SpecStateInterrupted
	SpecStateTimedout
)

var ssEnumSupport = NewEnumSupport(map[uint]string{uint(SpecStateInvalid): "INVALID SPEC STATE", uint(SpecStatePending): "pending", uint(SpecStateSkipped): "skipped", uint(SpecStatePassed): "passed", uint(SpecStateFailed): "failed", uint(SpecStateAborted): "aborted", uint(SpecStatePanicked): "panicked", uint(SpecStateInterrupted): "interrupted", uint(SpecStateTimedout): "timedout"})

func (ss SpecState) String() string {
	panic("stub")
}
func (ss SpecState) GomegaString() string {
	panic("stub")
}
func (ss *SpecState) UnmarshalJSON(b []byte) error {
	panic("stub")
}
func (ss SpecState) MarshalJSON() ([]byte, error) {
	panic("stub")
}

var SpecStateFailureStates = SpecStateFailed | SpecStateTimedout | SpecStateAborted | SpecStatePanicked | SpecStateInterrupted

func (ss SpecState) Is(states SpecState) bool {
	panic("stub")
}

type ProgressReport struct {
	Message                    string           `json:",omitempty"`
	ParallelProcess            int              `json:",omitempty"`
	RunningInParallel          bool             `json:",omitempty"`
	ContainerHierarchyTexts    []string         `json:",omitempty"`
	LeafNodeText               string           `json:",omitempty"`
	LeafNodeLocation           CodeLocation     `json:",omitempty"`
	SpecStartTime              time.Time        `json:",omitempty"`
	CurrentNodeType            NodeType         `json:",omitempty"`
	CurrentNodeText            string           `json:",omitempty"`
	CurrentNodeLocation        CodeLocation     `json:",omitempty"`
	CurrentNodeStartTime       time.Time        `json:",omitempty"`
	CurrentStepText            string           `json:",omitempty"`
	CurrentStepLocation        CodeLocation     `json:",omitempty"`
	CurrentStepStartTime       time.Time        `json:",omitempty"`
	AdditionalReports          []string         `json:",omitempty"`
	CapturedGinkgoWriterOutput string           `json:",omitempty"`
	TimelineLocation           TimelineLocation `json:",omitempty"`
	Goroutines                 []Goroutine      `json:",omitempty"`
}

func (pr ProgressReport) IsZero() bool {
	panic("stub")
}
func (pr ProgressReport) Time() time.Time {
	panic("stub")
}
func (pr ProgressReport) SpecGoroutine() Goroutine {
	panic("stub")
}
func (pr ProgressReport) HighlightedGoroutines() []Goroutine {
	panic("stub")
}
func (pr ProgressReport) OtherGoroutines() []Goroutine {
	panic("stub")
}
func (pr ProgressReport) WithoutCapturedGinkgoWriterOutput() ProgressReport {
	panic("stub")
}
func (pr ProgressReport) WithoutOtherGoroutines() ProgressReport {
	panic("stub")
}
func (pr ProgressReport) GetTimelineLocation() TimelineLocation {
	panic("stub")
}

type Goroutine struct {
	ID              uint64
	State           string
	Stack           []FunctionCall
	IsSpecGoroutine bool
}

func (g Goroutine) IsZero() bool {
	panic("stub")
}
func (g Goroutine) HasHighlights() bool {
	panic("stub")
}

type FunctionCall struct {
	Function        string
	Filename        string
	Line            int
	Highlight       bool     `json:",omitempty"`
	Source          []string `json:",omitempty"`
	SourceHighlight int      `json:",omitempty"`
}
type NodeType uint

const (
	NodeTypeInvalid   NodeType = 0
	NodeTypeContainer NodeType = 1 << iota
	NodeTypeIt
	NodeTypeBeforeEach
	NodeTypeJustBeforeEach
	NodeTypeAfterEach
	NodeTypeJustAfterEach
	NodeTypeBeforeAll
	NodeTypeAfterAll
	NodeTypeBeforeSuite
	NodeTypeSynchronizedBeforeSuite
	NodeTypeAfterSuite
	NodeTypeSynchronizedAfterSuite
	NodeTypeReportBeforeEach
	NodeTypeReportAfterEach
	NodeTypeReportBeforeSuite
	NodeTypeReportAfterSuite
	NodeTypeCleanupInvalid
	NodeTypeCleanupAfterEach
	NodeTypeCleanupAfterAll
	NodeTypeCleanupAfterSuite
)

var NodeTypesForContainerAndIt = NodeTypeContainer | NodeTypeIt
var NodeTypesForSuiteLevelNodes = NodeTypeBeforeSuite | NodeTypeSynchronizedBeforeSuite | NodeTypeAfterSuite | NodeTypeSynchronizedAfterSuite | NodeTypeReportBeforeSuite | NodeTypeReportAfterSuite | NodeTypeCleanupAfterSuite
var NodeTypesAllowedDuringCleanupInterrupt = NodeTypeAfterEach | NodeTypeJustAfterEach | NodeTypeAfterAll | NodeTypeAfterSuite | NodeTypeSynchronizedAfterSuite | NodeTypeCleanupAfterEach | NodeTypeCleanupAfterAll | NodeTypeCleanupAfterSuite
var NodeTypesAllowedDuringReportInterrupt = NodeTypeReportBeforeEach | NodeTypeReportAfterEach | NodeTypeReportBeforeSuite | NodeTypeReportAfterSuite
var ntEnumSupport = NewEnumSupport(map[uint]string{uint(NodeTypeInvalid): "INVALID NODE TYPE", uint(NodeTypeContainer): "Container", uint(NodeTypeIt): "It", uint(NodeTypeBeforeEach): "BeforeEach", uint(NodeTypeJustBeforeEach): "JustBeforeEach", uint(NodeTypeAfterEach): "AfterEach", uint(NodeTypeJustAfterEach): "JustAfterEach", uint(NodeTypeBeforeAll): "BeforeAll", uint(NodeTypeAfterAll): "AfterAll", uint(NodeTypeBeforeSuite): "BeforeSuite", uint(NodeTypeSynchronizedBeforeSuite): "SynchronizedBeforeSuite", uint(NodeTypeAfterSuite): "AfterSuite", uint(NodeTypeSynchronizedAfterSuite): "SynchronizedAfterSuite", uint(NodeTypeReportBeforeEach): "ReportBeforeEach", uint(NodeTypeReportAfterEach): "ReportAfterEach", uint(NodeTypeReportBeforeSuite): "ReportBeforeSuite", uint(NodeTypeReportAfterSuite): "ReportAfterSuite", uint(NodeTypeCleanupInvalid): "DeferCleanup", uint(NodeTypeCleanupAfterEach): "DeferCleanup (Each)", uint(NodeTypeCleanupAfterAll): "DeferCleanup (All)", uint(NodeTypeCleanupAfterSuite): "DeferCleanup (Suite)"})

func (nt NodeType) String() string {
	panic("stub")
}
func (nt *NodeType) UnmarshalJSON(b []byte) error {
	panic("stub")
}
func (nt NodeType) MarshalJSON() ([]byte, error) {
	panic("stub")
}
func (nt NodeType) Is(nodeTypes NodeType) bool {
	panic("stub")
}

type SpecEvent struct {
	SpecEventType    SpecEventType
	CodeLocation     CodeLocation
	TimelineLocation TimelineLocation
	Message          string        `json:",omitempty"`
	Duration         time.Duration `json:",omitempty"`
	NodeType         NodeType      `json:",omitempty"`
	Attempt          int           `json:",omitempty"`
}

func (se SpecEvent) GetTimelineLocation() TimelineLocation {
	panic("stub")
}
func (se SpecEvent) IsOnlyVisibleAtVeryVerbose() bool {
	panic("stub")
}
func (se SpecEvent) GomegaString() string {
	panic("stub")
}

type SpecEvents []SpecEvent

func (se SpecEvents) WithType(seType SpecEventType) SpecEvents {
	panic("stub")
}

type SpecEventType uint

const (
	SpecEventInvalid SpecEventType = 0
	SpecEventByStart SpecEventType = 1 << iota
	SpecEventByEnd
	SpecEventNodeStart
	SpecEventNodeEnd
	SpecEventSpecRepeat
	SpecEventSpecRetry
)

var seEnumSupport = NewEnumSupport(map[uint]string{uint(SpecEventInvalid): "INVALID SPEC EVENT", uint(SpecEventByStart): "By", uint(SpecEventByEnd): "By (End)", uint(SpecEventNodeStart): "Node", uint(SpecEventNodeEnd): "Node (End)", uint(SpecEventSpecRepeat): "Repeat", uint(SpecEventSpecRetry): "Retry"})

func (se SpecEventType) String() string {
	panic("stub")
}
func (se *SpecEventType) UnmarshalJSON(b []byte) error {
	panic("stub")
}
func (se SpecEventType) MarshalJSON() ([]byte, error) {
	panic("stub")
}
func (se SpecEventType) Is(specEventTypes SpecEventType) bool {
	panic("stub")
}
