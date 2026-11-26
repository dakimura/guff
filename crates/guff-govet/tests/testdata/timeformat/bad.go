package p

import "time"

func f() {
	_, _ = time.Parse("2006-02-01", "2020-01-15")
}
