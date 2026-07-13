package p

import "time"

func f() {
	_, _ = time.Parse("2006-01-02", "2020-01-15")
}
