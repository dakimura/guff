package time

type Duration int64

const Second Duration = 1

type Time struct{}

func Now() Time        { return Time{} }
func Sleep(d Duration) {}
