package testing

// Enough of testing for the analyzer: the forbidden methods are the ones that
// end in runtime.Goexit, and Run is what opens a subtest region.
type common struct{}

func (c *common) Error(args ...any)              {}
func (c *common) Errorf(format string, a ...any) {}
func (c *common) FailNow()                       {}
func (c *common) Fatal(args ...any)              {}
func (c *common) Fatalf(format string, a ...any) {}
func (c *common) Skip(args ...any)               {}
func (c *common) SkipNow()                       {}
func (c *common) Skipf(format string, a ...any)  {}

type T struct{ common }

func (t *T) Run(name string, f func(t *T)) bool { return true }

type B struct{ common }

func (b *B) Run(name string, f func(b *B)) bool { return true }
