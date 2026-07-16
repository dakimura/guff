package testing

type T struct{}

func (t *T) Fatal(args ...any) {}
func (t *T) Helper()           {}
