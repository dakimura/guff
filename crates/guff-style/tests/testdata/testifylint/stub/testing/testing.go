package testing

type T struct{}

func (t *T) Errorf(format string, args ...interface{}) {}
func (t *T) FailNow()                                  {}
