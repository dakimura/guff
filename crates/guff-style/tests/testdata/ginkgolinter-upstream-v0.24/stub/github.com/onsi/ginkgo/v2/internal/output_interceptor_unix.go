package internal

import (
	"os"
)

func NewOutputInterceptor() OutputInterceptor {
	panic("stub")
}

type dupSyscallOutputInterceptorImpl struct{}

func (impl *dupSyscallOutputInterceptorImpl) CreateStdoutStderrClones() (*os.File, *os.File) {
	panic("stub")
}
func (impl *dupSyscallOutputInterceptorImpl) ConnectPipeToStdoutStderr(pipeWriter *os.File) {
	panic("stub")
}
func (impl *dupSyscallOutputInterceptorImpl) RestoreStdoutStderrFromClones(stdoutClone *os.File, stderrClone *os.File) {
	panic("stub")
}
func (impl *dupSyscallOutputInterceptorImpl) ShutdownClones(stdoutClone *os.File, stderrClone *os.File) {
	panic("stub")
}
