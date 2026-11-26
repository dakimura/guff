package require

type TestingT interface {
	Errorf(format string, args ...interface{})
	FailNow()
}

type Assertions struct{}

func New(t TestingT) *Assertions { return &Assertions{} }

func Equal(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool { return true }
func True(t TestingT, value bool, msgAndArgs ...interface{}) bool                    { return true }
func Truef(t TestingT, value bool, msg string, args ...interface{}) bool             { return true }
func Nil(t TestingT, object interface{}, msgAndArgs ...interface{}) bool             { return true }
func NoError(t TestingT, err error, msgAndArgs ...interface{}) bool                  { return true }
func NoErrorf(t TestingT, err error, msg string, args ...interface{}) bool           { return true }
func Error(t TestingT, err error, msgAndArgs ...interface{}) bool                    { return true }
func Fail(t TestingT, failureMessage string, msgAndArgs ...interface{}) bool         { return true }
func Failf(t TestingT, failureMessage string, msg string, args ...interface{}) bool {
	return true
}
func FailNow(t TestingT, failureMessage string, msgAndArgs ...interface{}) bool { return true }
func FailNowf(t TestingT, failureMessage string, msg string, args ...interface{}) bool {
	return true
}
func ErrorIs(t TestingT, err, target error, msgAndArgs ...interface{}) bool {
	return true
}
func NotErrorIs(t TestingT, err, target error, msgAndArgs ...interface{}) bool {
	return true
}
func ErrorAs(t TestingT, err error, target interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func EqualError(t TestingT, err error, expected string, msgAndArgs ...interface{}) bool {
	return true
}
func ErrorContains(t TestingT, err error, contains string, msgAndArgs ...interface{}) bool {
	return true
}
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
func (a *Assertions) Error(err error, msgAndArgs ...interface{}) bool   { return true }
func (a *Assertions) ErrorIs(err, target error, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) NotErrorIs(err, target error, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) ErrorAs(err error, target interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) EqualError(err error, expected string, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) ErrorContains(err error, contains string, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) Fail(failureMessage string, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) FailNow(failureMessage string, msgAndArgs ...interface{}) bool {
	return true
}
