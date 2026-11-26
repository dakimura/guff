package modernize

// basic: var + loop += + rvalue use
func builderBasic() {
	var s string
	s += "before"
	for i := 0; i < 10; i++ {
		s += "in"
	}
	s += "after"
	print(s)
}

// short decl
func builderShort() {
	s := "a"
	for i := 0; i < 10; i++ {
		s += "b"
	}
	print(s)
}

// empty short decl
func builderEmptyShort() {
	s := ""
	for i := 0; i < 10; i++ {
		s += "b"
	}
	print(s)
}

// paren var decl where s is last
func builderParenLast(slice []string) string {
	var (
		msg string
	)
	for _, s := range slice {
		msg += s
	}
	return msg
}

// nope: += only outside a loop
func builderNoLoop() {
	var s string
	s += "a"
	s += "b"
	print(s)
}

// nope: declaration in if init (not an unrestricted stmt list)
func builderIfInit() {
	if s := "a"; true {
		for i := 0; i < 10; i++ {
			s += "x"
		}
		print(s)
	}
}

// nope: direct assignment (not only +=)
func builderDirectAssign(x string) string {
	var s string
	s = x
	for i := 0; i < 3; i++ {
		s += x
	}
	return s
}

// nope: s is not last in paren var decl
func builderNotLast() {
	var (
		str   = "hello"
		after int
	)
	for i := 0; i < 100; i++ {
		str += "!"
	}
	println(str, after)
}

// nope: += after an rvalue use
func builderAfterRvalue() {
	var s string
	for _, x := range []string{"a", "b"} {
		s += x
	}
	print(s)
	s += "extra"
	print(s)
}
