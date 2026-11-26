package gbytes

import (
	"regexp"
)

type BufferProvider interface{ Buffer() *Buffer }

func Say(expected string, args ...any) *sayMatcher {
	panic("stub")
}

type sayMatcher struct {
	re              *regexp.Regexp
	receivedSayings []byte
}

func (m *sayMatcher) buffer(actual any) (*Buffer, bool) {
	panic("stub")
}
func (m *sayMatcher) Match(actual any) (success bool, err error) {
	panic("stub")
}
func (m *sayMatcher) FailureMessage(actual any) (message string) {
	panic("stub")
}
func (m *sayMatcher) NegatedFailureMessage(actual any) (message string) {
	panic("stub")
}
func (m *sayMatcher) MatchMayChangeInTheFuture(actual any) bool {
	panic("stub")
}
