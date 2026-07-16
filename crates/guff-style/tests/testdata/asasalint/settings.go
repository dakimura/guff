package asasalint_settings

func Append(dst []any, src ...any) []any {
	return append(dst, src...)
}

func use(dst, src []any) {
	_ = Append(dst, src) // want when Append is not excluded
}
