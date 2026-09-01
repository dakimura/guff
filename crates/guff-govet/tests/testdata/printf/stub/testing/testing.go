package testing

type common struct{}

func (c *common) Errorf(format string, args ...any) {}

func (c *common) Logf(format string, args ...any) {}

func (c *common) Fatalf(format string, args ...any) {}

type T struct{ common }
