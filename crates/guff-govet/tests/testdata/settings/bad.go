// Package settings is the fixture for govet's *selector* keys, not for any one
// analyzer: it trips exactly three analyzers, each implemented by guff and each
// on golangci-lint's default list, so that `enable` / `disable` / `enable-all`
// / `disable-all` can be read off as a subtraction from one baseline.
//
// It must stay free of anything the sixteen analyzers guff does not implement
// would report, because `enable-all` turns those on too — in particular no
// shadowed variables (shadow), no struct that could be packed tighter
// (fieldalignment), no nil comparison a CFG could prove (nilness) and no
// sync.WaitGroup (waitgroup). None of those four is on the default list, so
// they are invisible until the `enable-all` case, which is exactly the kind of
// coupling that makes a golden move for a reason unrelated to its subject.
package settings

func Assign(x int) int {
	x = x // assign
	return x
}

func Shift() int8 {
	var i int8 = 1
	return i << 10 // shift
}

func Unreachable() int {
	return 1
	return 2 // unreachable
}
