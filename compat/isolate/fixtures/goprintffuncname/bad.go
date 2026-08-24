package p

func printfLikeFunc(format string, args ...interface{}) {}

// goprintffuncname reports each printf-like function whose name lacks the `f`,
// so a second one is a second finding.
func Warn(format string, args ...interface{}) {
	_ = format
	_ = args
}
