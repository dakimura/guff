package ok

// The comma-ok forms, the `any` assertion (upstream's `isAny`), and a type
// switch — all silent.

func commaOkAssign(a any) {
	if v, ok := a.(int); ok {
		_ = v
	}
}

func commaOkSpec(a any) {
	var v, ok = a.(int)
	_, _ = v, ok
}

func assertToAny(a any) {
	v := a.(any)
	_ = v
}

func typeSwitch(a any) string {
	switch a.(type) {
	case int:
		return "int"
	default:
		return "other"
	}
}
