// Package testing stands in for the real one. `T` embeds `common` exactly as
// upstream does, because the mapping is keyed on `(*testing.common).Error` —
// the *promoted* method — and not on `(*testing.T).Error`.
package testing

type common struct{}

func (c *common) Error(args ...interface{})                 {}
func (c *common) Errorf(format string, args ...interface{}) {}
func (c *common) Fatal(args ...interface{})                 {}
func (c *common) Fatalf(format string, args ...interface{}) {}
func (c *common) Log(args ...interface{})                   {}
func (c *common) Logf(format string, args ...interface{})   {}
func (c *common) Skip(args ...interface{})                  {}
func (c *common) Skipf(format string, args ...interface{})  {}

type T struct{ common }

type TB interface {
	Error(args ...interface{})
	Errorf(format string, args ...interface{})
	Fatal(args ...interface{})
	Fatalf(format string, args ...interface{})
	Log(args ...interface{})
	Logf(format string, args ...interface{})
	Skip(args ...interface{})
	Skipf(format string, args ...interface{})
}
