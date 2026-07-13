package main

func fn1() {
	var x *int
	_ = *x
	if x != nil {
		nop()
	}
}

func fn2() {
	var x *int
	if x == nil {
		nop()
	}
	_ = *x
}

func nop() {}
