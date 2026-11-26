package parallel_support

import (
	"io"
	"sync"
	"github.com/onsi/ginkgo/v2/reporters"
	"github.com/onsi/ginkgo/v2/types"
)

type Void struct{}

var voidReceiver *Void = &Void{}
var voidSender Void

type ServerHandler struct {
	done                   chan any
	outputDestination      io.Writer
	reporter               reporters.Reporter
	alives                 []func() bool
	lock                   *sync.Mutex
	beforeSuiteState       BeforeSuiteState
	reportBeforeSuiteState types.SpecState
	parallelTotal          int
	counter                int
	counterLock            *sync.Mutex
	shouldAbort            bool
	numSuiteDidBegins      int
	numSuiteDidEnds        int
	aggregatedReport       types.Report
	reportHoldingArea      []types.SpecReport
}

func newServerHandler(parallelTotal int, reporter reporters.Reporter) *ServerHandler {
	panic("stub")
}
func (handler *ServerHandler) SpecSuiteWillBegin(report types.Report, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) DidRun(report types.SpecReport, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) SpecSuiteDidEnd(report types.Report, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) EmitOutput(output []byte, n *int) error {
	panic("stub")
}
func (handler *ServerHandler) EmitProgressReport(report types.ProgressReport, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) registerAlive(proc int, alive func() bool) {
	panic("stub")
}
func (handler *ServerHandler) procIsAlive(proc int) bool {
	panic("stub")
}
func (handler *ServerHandler) haveNonprimaryProcsFinished() bool {
	panic("stub")
}
func (handler *ServerHandler) ReportBeforeSuiteCompleted(reportBeforeSuiteState types.SpecState, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) ReportBeforeSuiteState(_ Void, reportBeforeSuiteState *types.SpecState) error {
	panic("stub")
}
func (handler *ServerHandler) BeforeSuiteCompleted(beforeSuiteState BeforeSuiteState, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) BeforeSuiteState(_ Void, beforeSuiteState *BeforeSuiteState) error {
	panic("stub")
}
func (handler *ServerHandler) HaveNonprimaryProcsFinished(_ Void, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) AggregatedNonprimaryProcsReport(_ Void, report *types.Report) error {
	panic("stub")
}
func (handler *ServerHandler) Counter(_ Void, counter *int) error {
	panic("stub")
}
func (handler *ServerHandler) Abort(_ Void, _ *Void) error {
	panic("stub")
}
func (handler *ServerHandler) ShouldAbort(_ Void, shouldAbort *bool) error {
	panic("stub")
}
