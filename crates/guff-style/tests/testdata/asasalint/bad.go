package asasalint_bad

// The pinned asasalint is two unchecked type assertions — `typ.(*types.Slice)`
// and `Elem().(*types.Interface)` — and neither unaliases. Under
// `gotypesalias=1`, the default since Go 1.23, `any` is an `*types.Alias`, so
// only code that spells `interface{}` on **both** sides is reported. Everything
// written with `any` lives in `ok.go`.

func A(args ...interface{}) int {
	return len(args)
}

func B(args ...interface{}) int {
	return A(args) // want
}

func errMsg(msg string, args ...interface{}) string {
	_ = msg
	return ""
}

func Err(msg string, args ...interface{}) string {
	return errMsg(msg, args) // want
}

func use() {
	_ = B([]interface{}{1, 2, 3})       // want
	_ = Err("x %s", []interface{}{"a"}) // want
}
