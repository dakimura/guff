package reporters

import (
	"io"
	"sync"
	"github.com/onsi/ginkgo/v2/formatter"
	"github.com/onsi/ginkgo/v2/types"
)

type DefaultReporter struct {
	conf                     types.ReporterConfig
	writer                   io.Writer
	lastCharWasNewline       bool
	lastEmissionWasDelimiter bool
	specDenoter              string
	retryDenoter             string
	formatter                formatter.Formatter
	fdHierarchy              []string
	runningInParallel        bool
	lock                     *sync.Mutex
}

func NewDefaultReporterUnderTest(conf types.ReporterConfig, writer io.Writer) *DefaultReporter {
	panic("stub")
}
func NewDefaultReporter(conf types.ReporterConfig, writer io.Writer) *DefaultReporter {
	panic("stub")
}
func (r *DefaultReporter) SuiteWillBegin(report types.Report) {
	panic("stub")
}
func (r *DefaultReporter) SuiteDidEnd(report types.Report) {
	panic("stub")
}
func (r *DefaultReporter) WillRun(report types.SpecReport) {
	panic("stub")
}
func (r *DefaultReporter) wrapTextBlock(sectionName string, fn func()) {
	panic("stub")
}
func (r *DefaultReporter) DidRun(report types.SpecReport) {
	panic("stub")
}
func (r *DefaultReporter) didRunFd(report types.SpecReport) {
	panic("stub")
}
func (r *DefaultReporter) highlightColorForState(state types.SpecState) string {
	panic("stub")
}
func (r *DefaultReporter) humanReadableState(state types.SpecState) string {
	panic("stub")
}
func (r *DefaultReporter) emitTimeline(indent uint, report types.SpecReport, timeline types.Timeline) {
	panic("stub")
}
func (r *DefaultReporter) EmitFailure(state types.SpecState, failure types.Failure) {
	panic("stub")
}
func (r *DefaultReporter) emitShortFailure(indent uint, state types.SpecState, failure types.Failure) {
	panic("stub")
}
func (r *DefaultReporter) emitFailure(indent uint, state types.SpecState, failure types.Failure, includeAdditionalFailure bool) {
	panic("stub")
}
func (r *DefaultReporter) EmitProgressReport(report types.ProgressReport) {
	panic("stub")
}
func (r *DefaultReporter) emitProgressReport(indent uint, emitGinkgoWriterOutput, emitGroup bool, report types.ProgressReport) {
	panic("stub")
}
func (r *DefaultReporter) EmitReportEntry(entry types.ReportEntry) {
	panic("stub")
}
func (r *DefaultReporter) emitReportEntry(indent uint, entry types.ReportEntry) {
	panic("stub")
}
func (r *DefaultReporter) EmitSpecEvent(event types.SpecEvent) {
	panic("stub")
}
func (r *DefaultReporter) emitSpecEvent(indent uint, event types.SpecEvent, includeLocation bool) {
	panic("stub")
}
func (r *DefaultReporter) emitGoroutines(indent uint, goroutines ...types.Goroutine) {
	panic("stub")
}
func (r *DefaultReporter) emitSource(indent uint, fc types.FunctionCall) {
	panic("stub")
}
func (r *DefaultReporter) emit(s string) {
	panic("stub")
}
func (r *DefaultReporter) emitBlock(s string) {
	panic("stub")
}
func (r *DefaultReporter) emitDelimiter(indent uint) {
	panic("stub")
}
func (r *DefaultReporter) _emit(s string, block bool, isDelimiter bool) {
	panic("stub")
}
func (r *DefaultReporter) f(format string, args ...any) string {
	panic("stub")
}
func (r *DefaultReporter) fi(indentation uint, format string, args ...any) string {
	panic("stub")
}
func (r *DefaultReporter) cycleJoin(elements []string, joiner string) string {
	panic("stub")
}
func (r *DefaultReporter) codeLocationBlock(report types.SpecReport, highlightColor string, veryVerbose bool, usePreciseFailureLocation bool) string {
	panic("stub")
}
