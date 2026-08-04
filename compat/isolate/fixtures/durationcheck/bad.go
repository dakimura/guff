package p

import "time"

func Bad(d time.Duration) time.Duration {
	return d * time.Second // duration * duration
}
