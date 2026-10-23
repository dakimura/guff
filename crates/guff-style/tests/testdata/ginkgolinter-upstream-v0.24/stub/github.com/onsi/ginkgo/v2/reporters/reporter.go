package reporters

import (
	"github.com/onsi/ginkgo/v2/types"
)

type Reporter interface {
	SuiteWillBegin(report types.Report)
	WillRun(report types.SpecReport)
	DidRun(report types.SpecReport)
	SuiteDidEnd(report types.Report)
	EmitFailure(state types.SpecState, failure types.Failure)
	EmitProgressReport(progressReport types.ProgressReport)
	EmitReportEntry(entry types.ReportEntry)
	EmitSpecEvent(event types.SpecEvent)
}
type NoopReporter struct{}

func (n NoopReporter) SuiteWillBegin(report types.Report) {
	panic("stub")
}
func (n NoopReporter) WillRun(report types.SpecReport) {
	panic("stub")
}
func (n NoopReporter) DidRun(report types.SpecReport) {
	panic("stub")
}
func (n NoopReporter) SuiteDidEnd(report types.Report) {
	panic("stub")
}
func (n NoopReporter) EmitFailure(state types.SpecState, failure types.Failure) {
	panic("stub")
}
func (n NoopReporter) EmitProgressReport(progressReport types.ProgressReport) {
	panic("stub")
}
func (n NoopReporter) EmitReportEntry(entry types.ReportEntry) {
	panic("stub")
}
func (n NoopReporter) EmitSpecEvent(event types.SpecEvent) {
	panic("stub")
}
