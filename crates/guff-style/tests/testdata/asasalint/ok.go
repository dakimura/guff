package asasalint_ok

import "fmt"

func A(args ...any) int {
	return len(args)
}

func AIface(args ...interface{}) int {
	return len(args)
}

func okSpread(args []any) int {
	return A(args...)
}

func okElements() int {
	return A(1, 2, 3)
}

func okFmt(args []any) {
	fmt.Println(args)
}

// Everything below would be reported if the check unaliased `any`. It does not,
// because upstream does not: `Elem().(*types.Interface)` fails on the alias.

func okAnyIntoAny(args []any) int {
	return A(args)
}

func okIfaceIntoAny(args []interface{}) int {
	return A(args)
}

func okAnyIntoIface(args []any) int {
	return AIface(args)
}

// An alias for the slice type: `typ.(*types.Slice)` fails on it too.
type IfaceSlice = []interface{}

func okAliasSlice(args IfaceSlice) int {
	return AIface(args)
}

// An alias for the element.
type AnyAlias = interface{}

func okElemAlias(args ...AnyAlias) int {
	return len(args)
}

func okElemAliasCall(args []interface{}) int {
	return okElemAlias(args)
}

// A *named* slice type is silent in both tools — a `Named` is not a
// `*types.Slice` either.
type NamedIfaceSlice []interface{}

func okNamedSlice(args NamedIfaceSlice) int {
	return AIface(args)
}
