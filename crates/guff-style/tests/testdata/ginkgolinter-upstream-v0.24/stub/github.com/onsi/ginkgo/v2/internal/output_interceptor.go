package internal

import (
	"io"
	"os"
	"time"
)

const BAILOUT_TIME = 1 * time.Second
const BAILOUT_MESSAGE = `Ginkgo detected an issue while intercepting output.

When running in parallel, Ginkgo captures stdout and stderr output
and attaches it to the running spec.  It looks like that process is getting
stuck for this suite.

This usually happens if you, or a library you are using, spin up an external
process and set cmd.Stdout = os.Stdout and/or cmd.Stderr = os.Stderr.  This
causes the external process to keep Ginkgo's output interceptor pipe open and
causes output interception to hang.

Ginkgo has detected this and shortcircuited the capture process.  The specs
will continue running after this message however output from the external
process that caused this issue will not be captured.

You have several options to fix this.  In preferred order they are:

1. Pass GinkgoWriter instead of os.Stdout or os.Stderr to your process.
2. Ensure your process exits before the current spec completes.  If your
process is long-lived and must cross spec boundaries, this option won't
work for you.
3. Pause Ginkgo's output interceptor before starting your process and then
resume it after.  Use PauseOutputInterception() and ResumeOutputInterception()
to do this.
4. Set --output-interceptor-mode=none when running your Ginkgo suite.  This will
turn off all output interception but allow specs to run in parallel without this
issue.  You may miss important output if you do this including output from Go's
race detector.

More details on issue #851 - https://github.com/onsi/ginkgo/issues/851
`

type OutputInterceptor interface {
	StartInterceptingOutput()
	StartInterceptingOutputAndForwardTo(io.Writer)
	StopInterceptingAndReturnOutput() string
	PauseIntercepting()
	ResumeIntercepting()
	Shutdown()
}
type NoopOutputInterceptor struct{}

func (interceptor NoopOutputInterceptor) StartInterceptingOutput() {
	panic("stub")
}
func (interceptor NoopOutputInterceptor) StartInterceptingOutputAndForwardTo(io.Writer) {
	panic("stub")
}
func (interceptor NoopOutputInterceptor) StopInterceptingAndReturnOutput() string {
	panic("stub")
}
func (interceptor NoopOutputInterceptor) PauseIntercepting() {
	panic("stub")
}
func (interceptor NoopOutputInterceptor) ResumeIntercepting() {
	panic("stub")
}
func (interceptor NoopOutputInterceptor) Shutdown() {
	panic("stub")
}

type pipePair struct {
	reader *os.File
	writer *os.File
}

func startPipeFactory(pipeChannel chan pipePair, shutdown chan any) {
	panic("stub")
}

type interceptorImplementation interface {
	CreateStdoutStderrClones() (*os.File, *os.File)
	ConnectPipeToStdoutStderr(*os.File)
	RestoreStdoutStderrFromClones(*os.File, *os.File)
	ShutdownClones(*os.File, *os.File)
}
type genericOutputInterceptor struct {
	intercepting       bool
	stdoutClone        *os.File
	stderrClone        *os.File
	pipe               pipePair
	shutdown           chan any
	emergencyBailout   chan any
	pipeChannel        chan pipePair
	interceptedContent chan string
	forwardTo          io.Writer
	accumulatedOutput  string
	implementation     interceptorImplementation
}

func (interceptor *genericOutputInterceptor) StartInterceptingOutput() {
	panic("stub")
}
func (interceptor *genericOutputInterceptor) StartInterceptingOutputAndForwardTo(w io.Writer) {
	panic("stub")
}
func (interceptor *genericOutputInterceptor) StopInterceptingAndReturnOutput() string {
	panic("stub")
}
func (interceptor *genericOutputInterceptor) ResumeIntercepting() {
	panic("stub")
}
func (interceptor *genericOutputInterceptor) PauseIntercepting() {
	panic("stub")
}
func (interceptor *genericOutputInterceptor) Shutdown() {
	panic("stub")
}
func NewOSGlobalReassigningOutputInterceptor() OutputInterceptor {
	panic("stub")
}

type osGlobalReassigningOutputInterceptorImpl struct{}

func (impl *osGlobalReassigningOutputInterceptorImpl) CreateStdoutStderrClones() (*os.File, *os.File) {
	panic("stub")
}
func (impl *osGlobalReassigningOutputInterceptorImpl) ConnectPipeToStdoutStderr(pipeWriter *os.File) {
	panic("stub")
}
func (impl *osGlobalReassigningOutputInterceptorImpl) RestoreStdoutStderrFromClones(stdoutClone *os.File, stderrClone *os.File) {
	panic("stub")
}
func (impl *osGlobalReassigningOutputInterceptorImpl) ShutdownClones(_ *os.File, _ *os.File) {
	panic("stub")
}
