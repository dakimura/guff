package errors

type stringError string

func (e stringError) Error() string { return string(e) }
func New(text string) error         { return stringError(text) }
