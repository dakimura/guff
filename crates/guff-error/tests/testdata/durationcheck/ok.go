package durationcheck

import "time"

func ok(n int) time.Duration {
	return time.Duration(n) * time.Second
}
