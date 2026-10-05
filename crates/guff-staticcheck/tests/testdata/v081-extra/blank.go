package extra

// staticcheck v0.8: ir.EnclosingFunction looks a declared function up by its
// name, and `func _()` is not a package member, so SA4031 and SA9008 (the two
// checks that ask) skip its body. The named twin is still checked.

func _(x interface{}) {
	if x, ok := x.(int); ok {
		_ = x
	} else {
		_ = x
	}
	b := new(int)
	if b != nil {
		_ = b
	}
}

func named(x interface{}) {
	if x, ok := x.(int); ok {
		_ = x
	} else {
		_ = x
	}
	b := new(int)
	if b != nil {
		_ = b
	}
}

func _() {
	f := func(x interface{}) {
		if x, ok := x.(int); ok {
			_ = x
		} else {
			_ = x
		}
	}
	f(nil)
}
