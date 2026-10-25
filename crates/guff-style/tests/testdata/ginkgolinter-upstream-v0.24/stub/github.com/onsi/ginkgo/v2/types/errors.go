package types

import (
	"reflect"
)

type GinkgoError struct {
	Heading      string
	Message      string
	DocLink      string
	CodeLocation CodeLocation
}

func (g GinkgoError) Error() string {
	panic("stub")
}

type ginkgoErrors struct{}

var GinkgoErrors = ginkgoErrors{}

func (g ginkgoErrors) UncaughtGinkgoPanic(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) RerunningSuite() error {
	panic("stub")
}
func (g ginkgoErrors) PushingNodeInRunPhase(nodeType NodeType, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) CaughtPanicDuringABuildPhase(caughtPanic any, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) SuiteNodeInNestedContext(nodeType NodeType, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) SuiteNodeDuringRunPhase(nodeType NodeType, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) MultipleBeforeSuiteNodes(nodeType NodeType, cl CodeLocation, earlierNodeType NodeType, earlierCodeLocation CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) MultipleAfterSuiteNodes(nodeType NodeType, cl CodeLocation, earlierNodeType NodeType, earlierCodeLocation CodeLocation) error {
	panic("stub")
}
func ginkgoErrorMultipleSuiteNodes(setupOrTeardown string, nodeType NodeType, cl CodeLocation, earlierNodeType NodeType, earlierCodeLocation CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidDecoratorForNodeType(cl CodeLocation, nodeType NodeType, decorator string) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidDeclarationOfFocusedAndPending(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidDeclarationOfFlakeAttemptsAndMustPassRepeatedly(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) UnknownDecorator(cl CodeLocation, nodeType NodeType, decorator any) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidBodyTypeForContainer(t reflect.Type, cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidBodyType(t reflect.Type, cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidBodyTypeForSynchronizedBeforeSuiteProc1(t reflect.Type, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidBodyTypeForSynchronizedBeforeSuiteAllProcs(t reflect.Type, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) MultipleBodyFunctions(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) MissingBodyFunction(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidTimeoutOrGracePeriodForNonContextNode(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidTimeoutOrGracePeriodForNonContextCleanupNode(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidSerialNodeInNonSerialOrderedContainer(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) SetupNodeNotInOrderedContainer(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidContinueOnFailureDecoration(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) DeferCleanupInvalidFunction(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) PushingCleanupNodeDuringTreeConstruction(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) PushingCleanupInReportingNode(cl CodeLocation, nodeType NodeType) error {
	panic("stub")
}
func (g ginkgoErrors) PushingCleanupInCleanupNode(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) TooManyReportEntryValues(cl CodeLocation, arg any) error {
	panic("stub")
}
func (g ginkgoErrors) AddReportEntryNotDuringRunPhase(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) ByNotDuringRunPhase(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidFileFilter(filter string) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidFileFilterRegularExpression(filter string, err error) error {
	panic("stub")
}
func (g ginkgoErrors) SyntaxErrorParsingLabelFilter(input string, location int, error string) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidLabel(label string, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidEmptyLabel(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidSemVerConstraint(semVerConstraint, errMsg string, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidEmptySemVerConstraint(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidEmptyComponentForSemVerConstraint(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) MultipleEntryBodyFunctionsForTable(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidEntryDescription(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) MissingParametersForTableFunction(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) IncorrectParameterTypeForTable(i int, name string, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) TooFewParametersToTableFunction(expected, actual int, kind string, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) TooManyParametersToTableFunction(expected, actual int, kind string, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) IncorrectParameterTypeToTableFunction(i int, expected, actual reflect.Type, kind string, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) IncorrectVariadicParameterTypeToTableFunction(expected, actual reflect.Type, kind string, cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) ContextsCannotBeUsedInSubtreeTables(cl CodeLocation) error {
	panic("stub")
}
func (g ginkgoErrors) AggregatedReportUnavailableDueToNodeDisappearing() error {
	panic("stub")
}
func (g ginkgoErrors) SynchronizedBeforeSuiteFailedOnProc1() error {
	panic("stub")
}
func (g ginkgoErrors) SynchronizedBeforeSuiteDisappearedOnProc1() error {
	panic("stub")
}
func (g ginkgoErrors) UnknownTypePassedToRunSpecs(value any) error {
	panic("stub")
}

var sharedParallelErrorMessage = "It looks like you are trying to run specs in parallel with go test.\nThis is unsupported and you should use the ginkgo CLI instead."

func (g ginkgoErrors) InvalidParallelTotalConfiguration() error {
	panic("stub")
}
func (g ginkgoErrors) InvalidParallelProcessConfiguration() error {
	panic("stub")
}
func (g ginkgoErrors) MissingParallelHostConfiguration() error {
	panic("stub")
}
func (g ginkgoErrors) UnreachableParallelHost(host string) error {
	panic("stub")
}
func (g ginkgoErrors) DryRunInParallelConfiguration() error {
	panic("stub")
}
func (g ginkgoErrors) GracePeriodCannotBeZero() error {
	panic("stub")
}
func (g ginkgoErrors) InvalidSleepOnFailureConfiguration() error {
	panic("stub")
}
func (g ginkgoErrors) SleepOnFailureInParallelConfiguration() error {
	panic("stub")
}
func (g ginkgoErrors) ConflictingVerbosityConfiguration() error {
	panic("stub")
}
func (g ginkgoErrors) InvalidOutputInterceptorModeConfiguration(value string) error {
	panic("stub")
}
func (g ginkgoErrors) InvalidGoFlagCount() error {
	panic("stub")
}
func (g ginkgoErrors) InvalidGoFlagParallel() error {
	panic("stub")
}
func (g ginkgoErrors) BothRepeatAndUntilItFails() error {
	panic("stub")
}
func (g ginkgoErrors) ExpectFilenameNotPath(flag string, path string) error {
	panic("stub")
}
func (g ginkgoErrors) FlagAfterPositionalParameter() error {
	panic("stub")
}
func (g ginkgoErrors) FailedToParseStackTrace(message string) error {
	panic("stub")
}
