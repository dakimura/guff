package reporters

import (
	"encoding/xml"
	"regexp"
	"github.com/onsi/ginkgo/v2/config"
	"github.com/onsi/ginkgo/v2/types"
)

type JunitReportConfig struct {
	OmitTimelinesForSpecState          types.SpecState
	OmitFailureMessageAttr             bool
	OmitCapturedStdOutErr              bool
	OmitSpecLabels                     bool
	OmitSpecSemVerConstraints          bool
	OmitSpecComponentSemVerConstraints bool
	OmitLeafNodeType                   bool
	OmitSuiteSetupNodes                bool
}
type JUnitTestSuites struct {
	XMLName    xml.Name         `xml:"testsuites"`
	Tests      int              `xml:"tests,attr"`
	Disabled   int              `xml:"disabled,attr"`
	Errors     int              `xml:"errors,attr"`
	Failures   int              `xml:"failures,attr"`
	Time       float64          `xml:"time,attr"`
	TestSuites []JUnitTestSuite `xml:"testsuite"`
}
type JUnitTestSuite struct {
	Name       string          `xml:"name,attr"`
	Package    string          `xml:"package,attr"`
	Tests      int             `xml:"tests,attr"`
	Disabled   int             `xml:"disabled,attr"`
	Skipped    int             `xml:"skipped,attr"`
	Errors     int             `xml:"errors,attr"`
	Failures   int             `xml:"failures,attr"`
	Time       float64         `xml:"time,attr"`
	Timestamp  string          `xml:"timestamp,attr"`
	Properties JUnitProperties `xml:"properties"`
	TestCases  []JUnitTestCase `xml:"testcase"`
}
type JUnitProperties struct {
	Properties []JUnitProperty `xml:"property"`
}

func (jup JUnitProperties) WithName(name string) string {
	panic("stub")
}

type JUnitProperty struct {
	Name  string `xml:"name,attr"`
	Value string `xml:"value,attr"`
}

var ownerRE = regexp.MustCompile(`(?i)^owner:(.*)$`)

type JUnitTestCase struct {
	Name      string        `xml:"name,attr"`
	Classname string        `xml:"classname,attr"`
	Status    string        `xml:"status,attr"`
	Time      float64       `xml:"time,attr"`
	Owner     string        `xml:"owner,attr,omitempty"`
	Skipped   *JUnitSkipped `xml:"skipped,omitempty"`
	Error     *JUnitError   `xml:"error,omitempty"`
	Failure   *JUnitFailure `xml:"failure,omitempty"`
	SystemOut string        `xml:"system-out,omitempty"`
	SystemErr string        `xml:"system-err,omitempty"`
}
type JUnitSkipped struct {
	Message string `xml:"message,attr"`
}
type JUnitError struct {
	Message     string `xml:"message,attr"`
	Type        string `xml:"type,attr"`
	Description string `xml:",chardata"`
}
type JUnitFailure struct {
	Message     string `xml:"message,attr"`
	Type        string `xml:"type,attr"`
	Description string `xml:",chardata"`
}

func GenerateJUnitReport(report types.Report, dst string) error {
	panic("stub")
}
func GenerateJUnitReportWithConfig(report types.Report, dst string, config JunitReportConfig) error {
	panic("stub")
}
func MergeAndCleanupJUnitReports(sources []string, dst string) ([]string, error) {
	panic("stub")
}
func failureDescriptionForUnstructuredReporters(spec types.SpecReport) string {
	panic("stub")
}
func systemErrForUnstructuredReporters(spec types.SpecReport) string {
	panic("stub")
}
func RenderTimeline(spec types.SpecReport, noColor bool) string {
	panic("stub")
}
func systemOutForUnstructuredReporters(spec types.SpecReport) string {
	panic("stub")
}
func formatComponentSemVerConstraintsToString(componentSemVerConstraints map[string][]string) string {
	panic("stub")
}

type JUnitReporter struct{}

func NewJUnitReporter(_ string) *JUnitReporter {
	panic("stub")
}
func (reporter *JUnitReporter) SuiteWillBegin(_ config.GinkgoConfigType, _ *types.SuiteSummary) {
	panic("stub")
}
func (reporter *JUnitReporter) BeforeSuiteDidRun(_ *types.SetupSummary) {
	panic("stub")
}
func (reporter *JUnitReporter) SpecWillRun(_ *types.SpecSummary) {
	panic("stub")
}
func (reporter *JUnitReporter) SpecDidComplete(_ *types.SpecSummary) {
	panic("stub")
}
func (reporter *JUnitReporter) AfterSuiteDidRun(_ *types.SetupSummary) {
	panic("stub")
}
func (reporter *JUnitReporter) SuiteDidEnd(_ *types.SuiteSummary) {
	panic("stub")
}
