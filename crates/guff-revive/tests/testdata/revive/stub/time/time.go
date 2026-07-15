package time

type Duration int64

const Second Duration = 1e9

type Time struct{}

func (t Time) Equal(u Time) bool { return false }

func Date(year int, month int, day, hour, min, sec, nsec int, loc *Location) Time {
	return Time{}
}

type Location struct{}

