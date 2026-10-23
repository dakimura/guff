package gexec

import (
	"io"
	"os"
	"os/exec"
	"sync"
	"github.com/onsi/gomega/gbytes"
)

const INVALID_EXIT_CODE = 254

type Session struct {
	Command  *exec.Cmd
	Out      *gbytes.Buffer
	Err      *gbytes.Buffer
	Exited   <-chan struct{}
	lock     *sync.Mutex
	exitCode int
}

func Start(command *exec.Cmd, outWriter io.Writer, errWriter io.Writer) (*Session, error) {
	panic("stub")
}
func (s *Session) Buffer() *gbytes.Buffer {
	panic("stub")
}
func (s *Session) ExitCode() int {
	panic("stub")
}
func (s *Session) Wait(timeout ...any) *Session {
	panic("stub")
}
func (s *Session) Kill() *Session {
	panic("stub")
}
func (s *Session) Interrupt() *Session {
	panic("stub")
}
func (s *Session) Terminate() *Session {
	panic("stub")
}
func (s *Session) Signal(signal os.Signal) *Session {
	panic("stub")
}
func (s *Session) monitorForExit(exited chan<- struct{}) {
	panic("stub")
}
func (s *Session) processIsAlive() bool {
	panic("stub")
}

var trackedSessions = []*Session{}
var trackedSessionsMutex = &sync.Mutex{}

func KillAndWait(timeout ...any) {
	panic("stub")
}
func TerminateAndWait(timeout ...any) {
	panic("stub")
}
func Kill() {
	panic("stub")
}
func Terminate() {
	panic("stub")
}
func Signal(signal os.Signal) {
	panic("stub")
}
func Interrupt() {
	panic("stub")
}
