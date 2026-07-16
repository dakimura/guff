package asasalint_ok

import "fmt"

func A(args ...any) int {
	return len(args)
}

func okSpread(args []any) int {
	return A(args...)
}

func okElements() int {
	return A(1, 2, 3)
}

func okFmt(args []any) {
	fmt.Println(args) // builtin exclusion
}

func Append(dst []any, src ...any) []any {
	return append(dst, src...)
}

func useAppend(dst, src []any) {
	_ = Append(dst, src...) // spread is fine
}
