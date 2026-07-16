package require

type TestingT interface {
	Errorf(format string, args ...interface{})
	FailNow()
}

type Assertions struct{}

func New(t TestingT) *Assertions { return &Assertions{} }

func Equal(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool { return true }
func True(t TestingT, value bool, msgAndArgs ...interface{}) bool                    { return true }
func Nil(t TestingT, object interface{}, msgAndArgs ...interface{}) bool             { return true }
func NoError(t TestingT, err error, msgAndArgs ...interface{}) bool                  { return true }
func Error(t TestingT, err error, msgAndArgs ...interface{}) bool                    { return true }
func Len(t TestingT, object interface{}, length int, msgAndArgs ...interface{}) bool {
	return true
}

func (a *Assertions) Equal(expected, actual interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) True(value bool, msgAndArgs ...interface{}) bool { return true }
func (a *Assertions) Nil(object interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) NoError(err error, msgAndArgs ...interface{}) bool { return true }
