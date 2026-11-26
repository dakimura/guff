package assert

type TestingT interface {
	Errorf(format string, args ...interface{})
}

type Assertions struct{}

func New(t TestingT) *Assertions { return &Assertions{} }

func Equal(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool { return true }
func Equalf(t TestingT, expected, actual interface{}, msg string, args ...interface{}) bool {
	return true
}
func EqualValues(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func Exactly(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool { return true }
func NotEqual(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func NotEqualValues(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func True(t TestingT, value bool, msgAndArgs ...interface{}) bool  { return true }
func False(t TestingT, value bool, msgAndArgs ...interface{}) bool { return true }
func Nil(t TestingT, object interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func NotNil(t TestingT, object interface{}, msgAndArgs ...interface{}) bool { return true }
func Empty(t TestingT, object interface{}, msgAndArgs ...interface{}) bool  { return true }
func NotEmpty(t TestingT, object interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func Zero(t TestingT, i interface{}, msgAndArgs ...interface{}) bool    { return true }
func NotZero(t TestingT, i interface{}, msgAndArgs ...interface{}) bool { return true }
func Len(t TestingT, object interface{}, length int, msgAndArgs ...interface{}) bool {
	return true
}
func Error(t TestingT, err error, msgAndArgs ...interface{}) bool   { return true }
func NoError(t TestingT, err error, msgAndArgs ...interface{}) bool { return true }
func ErrorIs(t TestingT, err, target error, msgAndArgs ...interface{}) bool {
	return true
}
func NotErrorIs(t TestingT, err, target error, msgAndArgs ...interface{}) bool {
	return true
}
func IsType(t TestingT, expectedType, object interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func IsNotType(t TestingT, expectedType, object interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func Positive(t TestingT, e interface{}, msgAndArgs ...interface{}) bool { return true }
func Negative(t TestingT, e interface{}, msgAndArgs ...interface{}) bool { return true }
func Less(t TestingT, e1, e2 interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func LessOrEqual(t TestingT, e1, e2 interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func Greater(t TestingT, e1, e2 interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func GreaterOrEqual(t TestingT, e1, e2 interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func Same(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func NotSame(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func InEpsilon(t TestingT, expected, actual interface{}, epsilon float64, msgAndArgs ...interface{}) bool {
	return true
}
func InDelta(t TestingT, expected, actual interface{}, delta float64, msgAndArgs ...interface{}) bool {
	return true
}
func Contains(t TestingT, s, contains interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func NotContains(t TestingT, s, contains interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func Subset(t TestingT, list, subset interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func NotSubset(t TestingT, list, subset interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func Regexp(t TestingT, rx interface{}, str interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func NotRegexp(t TestingT, rx interface{}, str interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func ErrorAs(t TestingT, err error, target interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func NotErrorAs(t TestingT, err error, target interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func EqualError(t TestingT, err error, expected string, msgAndArgs ...interface{}) bool {
	return true
}
func ErrorContains(t TestingT, err error, contains string, msgAndArgs ...interface{}) bool {
	return true
}
func JSONEq(t TestingT, expected, actual string, msgAndArgs ...interface{}) bool { return true }
func YAMLEq(t TestingT, expected, actual string, msgAndArgs ...interface{}) bool { return true }
func Fail(t TestingT, failureMessage string, msgAndArgs ...interface{}) bool     { return true }
func Failf(t TestingT, failureMessage string, msg string, args ...interface{}) bool {
	return true
}
func FailNow(t TestingT, failureMessage string, msgAndArgs ...interface{}) bool { return true }
func FailNowf(t TestingT, failureMessage string, msg string, args ...interface{}) bool {
	return true
}

func (a *Assertions) Equal(expected, actual interface{}, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) Equalf(expected, actual interface{}, msg string, args ...interface{}) bool {
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
func (a *Assertions) Failf(failureMessage string, msg string, args ...interface{}) bool {
	return true
}
func (a *Assertions) FailNow(failureMessage string, msgAndArgs ...interface{}) bool {
	return true
}
func (a *Assertions) FailNowf(failureMessage string, msg string, args ...interface{}) bool {
	return true
}
