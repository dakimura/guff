package internal

import (
	"fmt"
	"time"
)

type PollingSignalErrorType int

const (
	PollingSignalErrorTypeStopTrying PollingSignalErrorType = iota
	PollingSignalErrorTypeTryAgainAfter
)

type PollingSignalError interface {
	error
	Wrap(err error) PollingSignalError
	Attach(description string, obj any) PollingSignalError
	Successfully() PollingSignalError
	Now()
}

var StopTrying = func(message string) PollingSignalError {
	return &PollingSignalErrorImpl{message: message, pollingSignalErrorType: PollingSignalErrorTypeStopTrying}
}
var TryAgainAfter = func(duration time.Duration) PollingSignalError {
	return &PollingSignalErrorImpl{message: fmt.Sprintf("told to try again after %s", duration), duration: duration, pollingSignalErrorType: PollingSignalErrorTypeTryAgainAfter}
}

type PollingSignalErrorAttachment struct {
	Description string
	Object      any
}
type PollingSignalErrorImpl struct {
	message                string
	wrappedErr             error
	pollingSignalErrorType PollingSignalErrorType
	duration               time.Duration
	successful             bool
	Attachments            []PollingSignalErrorAttachment
}

func (s *PollingSignalErrorImpl) Wrap(err error) PollingSignalError {
	panic("stub")
}
func (s *PollingSignalErrorImpl) Attach(description string, obj any) PollingSignalError {
	panic("stub")
}
func (s *PollingSignalErrorImpl) Error() string {
	panic("stub")
}
func (s *PollingSignalErrorImpl) Unwrap() error {
	panic("stub")
}
func (s *PollingSignalErrorImpl) Successfully() PollingSignalError {
	panic("stub")
}
func (s *PollingSignalErrorImpl) Now() {
	panic("stub")
}
func (s *PollingSignalErrorImpl) IsStopTrying() bool {
	panic("stub")
}
func (s *PollingSignalErrorImpl) IsSuccessful() bool {
	panic("stub")
}
func (s *PollingSignalErrorImpl) IsTryAgainAfter() bool {
	panic("stub")
}
func (s *PollingSignalErrorImpl) TryAgainDuration() time.Duration {
	panic("stub")
}
func AsPollingSignalError(actual any) (*PollingSignalErrorImpl, bool) {
	panic("stub")
}
