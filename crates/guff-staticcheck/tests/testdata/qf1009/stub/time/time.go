package time

type Time struct{}

func (Time) Equal(Time) bool { return false }
