package time

type Duration int64

const Second Duration = 1e9

type Timer struct{ C <-chan Duration }

func NewTimer(d Duration) *Timer {
	return &Timer{C: make(chan Duration)}
}

func (t *Timer) Reset(d Duration) bool { return false }
