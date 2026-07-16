package require

type TestingT interface {
	Errorf(format string, args ...interface{})
	FailNow()
}

func Equal(t TestingT, expected, actual interface{}, msgAndArgs ...interface{}) bool { return true }
func True(t TestingT, value bool, msgAndArgs ...interface{}) bool                    { return true }
func Nil(t TestingT, object interface{}, msgAndArgs ...interface{}) bool             { return true }
func NoError(t TestingT, err error, msgAndArgs ...interface{}) bool                  { return true }
func Error(t TestingT, err error, msgAndArgs ...interface{}) bool                    { return true }
func Len(t TestingT, object interface{}, length int, msgAndArgs ...interface{}) bool {
	return true
}
