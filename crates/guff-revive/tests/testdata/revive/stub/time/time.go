package time

type Duration int64

const Second Duration = 1e9

type Time struct{}

func Now() Time { return Time{} }

func (t Time) Equal(u Time) bool { return false }

func (t Time) Unix() int64 { return 0 }

func (t Time) UnixMilli() int64 { return 0 }

func Date(year int, month int, day, hour, min, sec, nsec int, loc *Location) Time {
	return Time{}
}

type Location struct{}

var UTC *Location

