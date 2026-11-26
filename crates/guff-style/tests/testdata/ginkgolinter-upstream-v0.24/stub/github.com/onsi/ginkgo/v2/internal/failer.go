package internal

import (
	"sync"
	"github.com/onsi/ginkgo/v2/types"
)

type Failer struct {
	lock    *sync.Mutex
	failure types.Failure
	state   types.SpecState
}

func NewFailer() *Failer {
	panic("stub")
}
func (f *Failer) GetState() types.SpecState {
	panic("stub")
}
func (f *Failer) GetFailure() types.Failure {
	panic("stub")
}
func (f *Failer) Panic(location types.CodeLocation, forwardedPanic any) {
	panic("stub")
}
func (f *Failer) Fail(message string, location types.CodeLocation) {
	panic("stub")
}
func (f *Failer) Skip(message string, location types.CodeLocation) {
	panic("stub")
}
func (f *Failer) AbortSuite(message string, location types.CodeLocation) {
	panic("stub")
}
func (f *Failer) Drain() (types.SpecState, types.Failure) {
	panic("stub")
}
