package testing

type T struct{}

func (t *T) Errorf(format string, args ...interface{}) {}
func (t *T) FailNow()                                  {}
func (t *T) Helper()                                   {}
func (t *T) Parallel()                                 {}
func (t *T) Run(name string, f func(t *T)) bool        { return true }
