package mock

type Arguments []interface{}

type TestingT interface {
	Logf(format string, args ...interface{})
	Errorf(format string, args ...interface{})
	FailNow()
}

type Call struct {
	Parent *Mock
}

func (c *Call) Return(returnArguments ...interface{}) *Call { return c }
func (c *Call) Once() *Call                                  { return c }
func (c *Call) Twice() *Call                                 { return c }
func (c *Call) Times(i int) *Call                            { return c }
func (c *Call) Run(fn func(args Arguments)) *Call            { return c }

type Mock struct{}

func (m *Mock) On(methodName string, arguments ...interface{}) *Call {
	return &Call{Parent: m}
}

func (m *Mock) Called(arguments ...interface{}) Arguments { return nil }
func (m *Mock) Test(t TestingT)                           {}
func (m *Mock) AssertExpectations(t TestingT) bool        { return true }

var Anything interface{} = "mock.Anything"
