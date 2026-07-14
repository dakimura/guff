package durationcheck

import "time"

func bad(d time.Duration) time.Duration {
	return d * time.Second
}
