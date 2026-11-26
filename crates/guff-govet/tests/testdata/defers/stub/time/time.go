package time

type Time struct{}

type Duration int64

func Now() Time {
	return Time{}
}

func Since(Time) Duration {
	return 0
}
