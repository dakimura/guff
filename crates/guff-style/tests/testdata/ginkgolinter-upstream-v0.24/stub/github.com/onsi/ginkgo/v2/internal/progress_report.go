package internal

import (
	"context"
	"time"
	"github.com/onsi/ginkgo/v2/types"
)

var _SOURCE_CACHE = map[string][]string{}

type ProgressSignalRegistrar func(func()) context.CancelFunc

func RegisterForProgressSignal(handler func()) context.CancelFunc {
	panic("stub")
}

type ProgressStepCursor struct {
	Text         string
	CodeLocation types.CodeLocation
	StartTime    time.Time
}

func NewProgressReport(isRunningInParallel bool, report types.SpecReport, currentNode Node, currentNodeStartTime time.Time, currentStep types.SpecEvent, gwOutput string, timelineLocation types.TimelineLocation, additionalReports []string, sourceRoots []string, includeAll bool) (types.ProgressReport, error) {
	panic("stub")
}
func extractRunningGoroutines() ([]types.Goroutine, error) {
	panic("stub")
}
func fetchSource(filename string, lineNumber int, span int, configuredSourceRoots []string) ([]string, int) {
	panic("stub")
}
