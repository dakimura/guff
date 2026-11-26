package parallel_support

import (
	"io"
	"net"
	"net/http"
	"github.com/onsi/ginkgo/v2/reporters"
)

type httpServer struct {
	listener net.Listener
	handler  *ServerHandler
}

func newHttpServer(parallelTotal int, reporter reporters.Reporter) (*httpServer, error) {
	panic("stub")
}
func (server *httpServer) Start() {
	panic("stub")
}
func (server *httpServer) Close() {
	panic("stub")
}
func (server *httpServer) Address() string {
	panic("stub")
}
func (server *httpServer) GetSuiteDone() chan any {
	panic("stub")
}
func (server *httpServer) GetOutputDestination() io.Writer {
	panic("stub")
}
func (server *httpServer) SetOutputDestination(w io.Writer) {
	panic("stub")
}
func (server *httpServer) RegisterAlive(node int, alive func() bool) {
	panic("stub")
}
func (server *httpServer) decode(writer http.ResponseWriter, request *http.Request, object any) bool {
	panic("stub")
}
func (server *httpServer) handleError(err error, writer http.ResponseWriter) bool {
	panic("stub")
}
func (server *httpServer) specSuiteWillBegin(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) didRun(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) specSuiteDidEnd(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) emitOutput(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) emitProgressReport(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleReportBeforeSuiteCompleted(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleReportBeforeSuiteState(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleBeforeSuiteCompleted(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleBeforeSuiteState(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleHaveNonprimaryProcsFinished(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleAggregatedNonprimaryProcsReport(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleCounter(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleUp(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
func (server *httpServer) handleAbort(writer http.ResponseWriter, request *http.Request) {
	panic("stub")
}
