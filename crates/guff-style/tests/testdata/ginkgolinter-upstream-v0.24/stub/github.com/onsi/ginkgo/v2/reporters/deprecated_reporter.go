package reporters

import (
	"github.com/onsi/ginkgo/v2/config"
	"github.com/onsi/ginkgo/v2/types"
)

type DeprecatedReporter interface {
	SuiteWillBegin(config config.GinkgoConfigType, summary *types.SuiteSummary)
	BeforeSuiteDidRun(setupSummary *types.SetupSummary)
	SpecWillRun(specSummary *types.SpecSummary)
	SpecDidComplete(specSummary *types.SpecSummary)
	AfterSuiteDidRun(setupSummary *types.SetupSummary)
	SuiteDidEnd(summary *types.SuiteSummary)
}

func ReportViaDeprecatedReporter(reporter DeprecatedReporter, report types.Report) {
	panic("stub")
}
func failureFor(spec types.SpecReport) types.DeprecatedSpecFailure {
	panic("stub")
}
