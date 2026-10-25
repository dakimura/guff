package parallel_support

import (
	"github.com/onsi/ginkgo/v2/types"
)

type httpClient struct{ serverHost string }

func newHttpClient(serverHost string) *httpClient {
	panic("stub")
}
func (client *httpClient) Connect() bool {
	panic("stub")
}
func (client *httpClient) Close() error {
	panic("stub")
}
func (client *httpClient) post(path string, data any) error {
	panic("stub")
}
func (client *httpClient) poll(path string, data any) error {
	panic("stub")
}
func (client *httpClient) PostSuiteWillBegin(report types.Report) error {
	panic("stub")
}
func (client *httpClient) PostDidRun(report types.SpecReport) error {
	panic("stub")
}
func (client *httpClient) PostSuiteDidEnd(report types.Report) error {
	panic("stub")
}
func (client *httpClient) PostEmitProgressReport(report types.ProgressReport) error {
	panic("stub")
}
func (client *httpClient) PostReportBeforeSuiteCompleted(state types.SpecState) error {
	panic("stub")
}
func (client *httpClient) BlockUntilReportBeforeSuiteCompleted() (types.SpecState, error) {
	panic("stub")
}
func (client *httpClient) PostSynchronizedBeforeSuiteCompleted(state types.SpecState, data []byte) error {
	panic("stub")
}
func (client *httpClient) BlockUntilSynchronizedBeforeSuiteData() (types.SpecState, []byte, error) {
	panic("stub")
}
func (client *httpClient) BlockUntilNonprimaryProcsHaveFinished() error {
	panic("stub")
}
func (client *httpClient) BlockUntilAggregatedNonprimaryProcsReport() (types.Report, error) {
	panic("stub")
}
func (client *httpClient) FetchNextCounter() (int, error) {
	panic("stub")
}
func (client *httpClient) PostAbort() error {
	panic("stub")
}
func (client *httpClient) ShouldAbort() bool {
	panic("stub")
}
func (client *httpClient) Write(p []byte) (int, error) {
	panic("stub")
}
