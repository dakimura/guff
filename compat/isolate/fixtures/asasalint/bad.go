package p

func A(args ...interface{}) int {
	return len(args)
}

func B(args []interface{}) int {
	return A(args)
}
