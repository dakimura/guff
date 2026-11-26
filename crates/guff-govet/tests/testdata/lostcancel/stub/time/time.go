package time

type Duration int64

const Second Duration = 1000000000

type Time struct{}

func Now() Time {
	return Time{}
}
