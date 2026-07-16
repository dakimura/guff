package gochecknoglobals

var myVar = 0

var myVar1, myVar2 = 1, 2

var Version string

var version22 string

var theVar = true

func localOk() {
	x := 1
	_ = x
}
