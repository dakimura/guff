package bad

import "time"

func bad() {
	now := time.Now()
	defer time.Since(now)
}
