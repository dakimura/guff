package usestdlibvars

import "time"

func OptionalBad() {
	_ = "Monday"
	_ = "January"
	_ = "2006-01-02"
	_ = "SHA-256"
	_ = "/_goRPC_"
	_ = "Read Committed"
	_ = "PSSWithSHA256"
	_ = "Bool"
	_ = time.Date(2023, 1, 2, 3, 4, 5, 0, time.UTC)
}
