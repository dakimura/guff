package ok

import "time"

func ok() {
	now := time.Now()
	defer func() {
		_ = time.Since(now)
	}()
	evalBefore := time.Since(now)
	_ = evalBefore
}
