package interrupt_handler

import (
	"os"
	"sync"
	"time"
	"github.com/onsi/ginkgo/v2/internal/parallel_support"
)

var ABORT_POLLING_INTERVAL = 500 * time.Millisecond

type InterruptCause uint

const (
	InterruptCauseInvalid InterruptCause = iota
	InterruptCauseSignal
	InterruptCauseAbortByOtherProcess
)

type InterruptLevel uint

const (
	InterruptLevelUninterrupted InterruptLevel = iota
	InterruptLevelCleanupAndReport
	InterruptLevelReportOnly
	InterruptLevelBailOut
)

func (ic InterruptCause) String() string {
	panic("stub")
}

type InterruptStatus struct {
	Channel chan any
	Level   InterruptLevel
	Cause   InterruptCause
}

func (s InterruptStatus) Interrupted() bool {
	panic("stub")
}
func (s InterruptStatus) Message() string {
	panic("stub")
}
func (s InterruptStatus) ShouldIncludeProgressReport() bool {
	panic("stub")
}

type InterruptHandlerInterface interface{ Status() InterruptStatus }
type InterruptHandler struct {
	c                 chan any
	lock              *sync.Mutex
	level             InterruptLevel
	cause             InterruptCause
	client            parallel_support.Client
	stop              chan any
	signals           []os.Signal
	requestAbortCheck chan any
}

func NewInterruptHandler(client parallel_support.Client, signals ...os.Signal) *InterruptHandler {
	panic("stub")
}
func (handler *InterruptHandler) Stop() {
	panic("stub")
}
func (handler *InterruptHandler) registerForInterrupts() {
	panic("stub")
}
func (handler *InterruptHandler) Status() InterruptStatus {
	panic("stub")
}
