package time

type Time struct{}

type Location struct{}

var UTC *Location

type Month int

const (
	January Month = 1
	February Month = 2
)

func (m Month) String() string { return "" }

type Weekday int

const Monday Weekday = 1

func (w Weekday) String() string { return "" }

const DateOnly = "2006-01-02"

func Date(year int, month Month, day, hour, min, sec, nsec int, loc *Location) Time {
	return Time{}
}
