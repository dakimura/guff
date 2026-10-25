package parallel_support

import (
	"net/rpc"
	"github.com/onsi/ginkgo/v2/types"
)

type rpcClient struct {
	serverHost string
	client     *rpc.Client
}

func newRPCClient(serverHost string) *rpcClient {
	panic("stub")
}
func (client *rpcClient) Connect() bool {
	panic("stub")
}
func (client *rpcClient) Close() error {
	panic("stub")
}
func (client *rpcClient) poll(method string, data any) error {
	panic("stub")
}
func (client *rpcClient) PostSuiteWillBegin(report types.Report) error {
	panic("stub")
}
func (client *rpcClient) PostDidRun(report types.SpecReport) error {
	panic("stub")
}
func (client *rpcClient) PostSuiteDidEnd(report types.Report) error {
	panic("stub")
}
func (client *rpcClient) Write(p []byte) (int, error) {
	panic("stub")
}
func (client *rpcClient) PostEmitProgressReport(report types.ProgressReport) error {
	panic("stub")
}
func (client *rpcClient) PostReportBeforeSuiteCompleted(state types.SpecState) error {
	panic("stub")
}
func (client *rpcClient) BlockUntilReportBeforeSuiteCompleted() (types.SpecState, error) {
	panic("stub")
}
func (client *rpcClient) PostSynchronizedBeforeSuiteCompleted(state types.SpecState, data []byte) error {
	panic("stub")
}
func (client *rpcClient) BlockUntilSynchronizedBeforeSuiteData() (types.SpecState, []byte, error) {
	panic("stub")
}
func (client *rpcClient) BlockUntilNonprimaryProcsHaveFinished() error {
	panic("stub")
}
func (client *rpcClient) BlockUntilAggregatedNonprimaryProcsReport() (types.Report, error) {
	panic("stub")
}
func (client *rpcClient) FetchNextCounter() (int, error) {
	panic("stub")
}
func (client *rpcClient) PostAbort() error {
	panic("stub")
}
func (client *rpcClient) ShouldAbort() bool {
	panic("stub")
}
