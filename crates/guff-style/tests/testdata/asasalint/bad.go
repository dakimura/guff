package asasalint_bad

func A(args ...any) int {
	return len(args)
}

func B(args ...any) int {
	return A(args) // want
}

func errMsg(msg string, args ...any) string {
	_ = msg
	return ""
}

func Err(msg string, args ...any) string {
	return errMsg(msg, args) // want
}

func use() {
	_ = B([]any{1, 2, 3}) // want
	_ = Err("x %s", []any{"a"}) // want
}
