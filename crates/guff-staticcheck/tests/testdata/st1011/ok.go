package pkg

import "time"

func ok(timeout time.Duration) time.Duration {
	var delay time.Duration
	deadline := time.Second
	return timeout + delay + deadline
}
