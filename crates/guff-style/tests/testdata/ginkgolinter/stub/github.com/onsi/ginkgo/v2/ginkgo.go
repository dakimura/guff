package ginkgo

func Describe(text string, args ...interface{}) bool { return false }
func FDescribe(text string, args ...interface{}) bool { return false }
func Context(text string, args ...interface{}) bool  { return false }
func FContext(text string, args ...interface{}) bool { return false }
func When(text string, args ...interface{}) bool     { return false }
func FWhen(text string, args ...interface{}) bool    { return false }
func It(text string, args ...interface{}) bool       { return false }
func FIt(text string, args ...interface{}) bool      { return false }
func BeforeEach(args ...interface{})                 {}
