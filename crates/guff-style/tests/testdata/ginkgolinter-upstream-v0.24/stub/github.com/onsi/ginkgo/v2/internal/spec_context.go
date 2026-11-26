package internal

import (
	"context"
	"github.com/onsi/ginkgo/v2/types"
)

type SpecContext interface {
	context.Context
	SpecReport() types.SpecReport
	AttachProgressReporter(func() string) func()
	WrappedContext() context.Context
}
type specContext struct {
	context.Context
	*ProgressReporterManager
	cancel context.CancelCauseFunc
	suite  *Suite
}

func NewSpecContext(suite *Suite) *specContext {
	panic("stub")
}
func (sc *specContext) SpecReport() types.SpecReport {
	panic("stub")
}
func (sc *specContext) WrappedContext() context.Context {
	panic("stub")
}
func wrapContextChain(ctx context.Context) SpecContext {
	panic("stub")
}
