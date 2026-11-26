package internal

import (
	"time"
)

type DurationBundle struct {
	EventuallyTimeout                       time.Duration
	EventuallyPollingInterval               time.Duration
	ConsistentlyDuration                    time.Duration
	ConsistentlyPollingInterval             time.Duration
	EnforceDefaultTimeoutsWhenUsingContexts bool
}

const (
	EventuallyTimeoutEnvVarName                       = "GOMEGA_DEFAULT_EVENTUALLY_TIMEOUT"
	EventuallyPollingIntervalEnvVarName               = "GOMEGA_DEFAULT_EVENTUALLY_POLLING_INTERVAL"
	ConsistentlyDurationEnvVarName                    = "GOMEGA_DEFAULT_CONSISTENTLY_DURATION"
	ConsistentlyPollingIntervalEnvVarName             = "GOMEGA_DEFAULT_CONSISTENTLY_POLLING_INTERVAL"
	EnforceDefaultTimeoutsWhenUsingContextsEnvVarName = "GOMEGA_ENFORCE_DEFAULT_TIMEOUTS_WHEN_USING_CONTEXTS"
)

func FetchDefaultDurationBundle() DurationBundle {
	panic("stub")
}
func durationFromEnv(key string, defaultDuration time.Duration) time.Duration {
	panic("stub")
}
func toDuration(input any) (time.Duration, error) {
	panic("stub")
}
