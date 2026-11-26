package testing

// Shaped like the real `testing`: the abort methods live on the unexported
// `common`, which `T` and `B` embed, so `t.Skip(…)` resolves to
// `(*testing.common).Skip` — the name `ctrlflow`'s no-return table is keyed on,
// and the one both tools see through the promotion.

type common struct{}

func (c *common) FailNow()                          {}
func (c *common) Fatal(args ...any)                 {}
func (c *common) Fatalf(format string, args ...any) {}
func (c *common) Skip(args ...any)                  {}
func (c *common) SkipNow()                          {}
func (c *common) Skipf(format string, args ...any)  {}

type T struct{ common }

type B struct{ common }

// TB is an interface, so a call through it has no static callee.
type TB interface {
	FailNow()
	Fatal(args ...any)
	Skip(args ...any)
	SkipNow()
}
