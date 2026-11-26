package testing

type common struct{}

func (c *common) Fatal(args ...interface{})   {}
func (c *common) FailNow()                    {}
func (c *common) Fatalf(format string, args ...interface{}) {}
func (c *common) Skip(args ...interface{})    {}
func (c *common) SkipNow()                    {}
func (c *common) Skipf(format string, args ...interface{}) {}

type T struct {
	common
}

type B struct {
	common
	N int
}

type M struct{}

func (m *M) Run() int { return 0 }
