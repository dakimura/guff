package time

type Location struct{}

var (
	UTC   = &Location{}
	Local = &Location{}
)
