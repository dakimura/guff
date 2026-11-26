package time

type Duration int64

func Tick(d Duration) <-chan Time {
	ch := make(chan Time)
	return ch
}

type Time struct{}
