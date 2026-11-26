package time

type Duration int64
type Time struct{}
func Now() Time { return Time{} }
func (Time) Sub(Time) Duration { return 0 }
func Since(Time) Duration { return 0 }
