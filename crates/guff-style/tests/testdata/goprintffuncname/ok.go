package goprintffuncnameok

func notPrintfFuncAtAll() {}

func printfLikeButWithStrings(format string, args ...string) {}

func prinfLikeFuncf(format string, args ...interface{}) {}

func prinfLikeFuncWithReturnValue(format string, args ...interface{}) string {
	return ""
}

func prinfLikeFuncWithAnotherFormatArgName(msg string, args ...interface{}) {}
