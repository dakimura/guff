package internal

import (
	"context"
	"sync"
)

type ProgressReporterManager struct {
	lock              *sync.Mutex
	progressReporters map[int]func() string
	prCounter         int
}

func NewProgressReporterManager() *ProgressReporterManager {
	panic("stub")
}
func (prm *ProgressReporterManager) AttachProgressReporter(reporter func() string) func() {
	panic("stub")
}
func (prm *ProgressReporterManager) QueryProgressReporters(ctx context.Context, failer *Failer) []string {
	panic("stub")
}
