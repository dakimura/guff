package usestdlibvars

import "time"

func OptionalOk() {
	_ = time.Monday.String()
	_ = time.January.String()
	_ = time.DateOnly
	_ = time.Date(2023, time.January, 2, 3, 4, 5, 0, time.UTC)
}
