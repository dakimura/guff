package time

type Time struct{}

func (Time) IsZero() bool { return false }

func (t Time) Equal(u Time) bool { return true }

func (t Time) Compare(u Time) int { return 0 }

type Location struct{}
