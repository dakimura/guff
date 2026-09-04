package asasalint_settings

// Spelled `interface{}`: the check never sees `any`, so the exclude list is
// only observable on this spelling. See `bad.go`.
func Append(dst []interface{}, src ...interface{}) []interface{} {
	return append(dst, src...)
}

func use(dst, src []interface{}) {
	_ = Append(dst, src) // want when Append is not excluded
}
