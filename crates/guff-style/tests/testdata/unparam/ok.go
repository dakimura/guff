package unparam

func allUsed(x int, y string) {
	_ = x
	println(y)
}

func explicitKeep(unused int) {
	_ = unused
}

func emptyBody(unused int) {}

func onlyReturn(unused int) {
	return
}

// Used as a value (callback); signature must stay — unused param is OK.
func asCallback(prefix string) {
	println("ok")
}

var callbacks = []func(string){asCallback}

func takesHandler(h func(x int)) { h(1) }

func callWithLit() {
	takesHandler(func(unused int) {
		println("handler")
	})
}

