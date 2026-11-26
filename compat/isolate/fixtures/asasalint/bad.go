package p

func A(args ...interface{}) int {
	return len(args)
}

func B(args []interface{}) int {
	return A(args)
}

// asasalint quotes the call it objects to, so each site is its own sentence —
// and a `...` spread is the negative it accepts.
func C(args []interface{}) int {
	return A(args, args)
}

func Spread(args []interface{}) int {
	return A(args...)
}
