package logr

type contextKey struct{}
type notFoundError struct{}

func (notFoundError) Error() string {
	panic("stub")
}
func (notFoundError) IsNotFound() bool {
	panic("stub")
}
