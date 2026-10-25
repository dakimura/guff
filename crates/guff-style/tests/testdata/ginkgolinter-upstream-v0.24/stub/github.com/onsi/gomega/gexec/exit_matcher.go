package gexec

func Exit(optionalExitCode ...int) *exitMatcher {
	panic("stub")
}

type exitMatcher struct {
	exitCode       int
	didExit        bool
	actualExitCode int
}
type Exiter interface{ ExitCode() int }

func (m *exitMatcher) Match(actual any) (success bool, err error) {
	panic("stub")
}
func (m *exitMatcher) FailureMessage(actual any) (message string) {
	panic("stub")
}
func (m *exitMatcher) NegatedFailureMessage(actual any) (message string) {
	panic("stub")
}
func (m *exitMatcher) MatchMayChangeInTheFuture(actual any) bool {
	panic("stub")
}
