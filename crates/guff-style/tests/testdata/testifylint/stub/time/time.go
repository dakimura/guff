package time

type Time struct{}

type Duration int64

type Location struct{}

func Now() Time { return Time{} }

func Date(year int, month int, day, hour, min, sec, nsec int, loc *Location) Time {
	return Time{}
}

func (Time) IsZero() bool { return false }

func (t Time) Equal(u Time) bool { return true }

func (t Time) Compare(u Time) int { return 0 }

func (t Time) UTC() Time { return Time{} }

func (t Time) Local() Time { return Time{} }

func (t Time) Round(d Duration) Time { return Time{} }

func (t Time) Truncate(d Duration) Time { return Time{} }

func (t Time) Add(d Duration) Time { return Time{} }

func (t Time) In(loc *Location) Time { return Time{} }

var UTC *Location
