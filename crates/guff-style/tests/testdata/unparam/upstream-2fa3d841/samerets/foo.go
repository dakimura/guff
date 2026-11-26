package foo

var DoWork func()

var Cond bool

func singleReturn() int {
	DoWork()
	return 3
}

func manyReturns(i int64) int {
	if i > 3 {
		DoWork()
		return 3
	}
	return 3
}

func manyReturnsDifferent(b bool) int {
	for Cond {
		DoWork()
		if b {
			return 4
		}
	}
	return 3
}

func manyReturnsMultiple() (b bool, s string) {
	if Cond {
		DoWork()
		return true, "foo"
	}
	return true, "foo"
}

func singleNilError() (bool, error) {
	DoWork()
	return true, nil
}

func manyNilError() (bool, error) {
	if Cond {
		DoWork()
		return false, nil
	}
	return true, nil
}

func manyReturnsForwarded(r rune) int {
	if r == '3' {
		return 5
	}
	DoWork()
	return 5
}

func forwarding(r rune) int {
	DoWork()
	return manyReturnsForwarded(r)
}

func doubleReturnForwarded() (int, error) {
	DoWork()
	return 5, nil
}

func forwardingDouble() (int, error) {
	DoWork()
	return doubleReturnForwarded()
}

func doubleReturnNotForwarded() (int, error) {
	DoWork()
	return 5, nil
}

func falseForwardingDoubleLen() int {
	DoWork()
	n, err := doubleReturnNotForwarded()
	println(err)
	return n
}

func falseForwardingDoubleOrder() (error, int) {
	DoWork()
	n, err := doubleReturnNotForwarded()
	return err, n
}

func neverForwarded() (int, error) {
	DoWork()
	return 5, nil
}

type FooType int

func (f FooType) neverForwardedPtrMethod() (int, error) {
	DoWork()
	return 5, nil
}

func neverForwarding() {
	DoWork()
	neverForwarded()
	f := new(FooType)
	f.neverForwardedPtrMethod()
}

func forwardingDoubleFuncLits(orig []byte) {
	rc := func(data []byte) (byte, error) {
		DoWork()
		return data[0], nil
	}
	_ = func() (byte, error) {
		return rc(orig)
	}
}

func doubleReturnForwardedDefer() (int, error) {
	DoWork()
	return 5, nil
}

func forwardingDoubleDefer() (int, error) {
	defer DoWork()
	return doubleReturnForwardedDefer()
}

func forwardingDoubleFuncLitsDefer(orig []byte) {
	rc := func(data []byte) (byte, error) {
		DoWork()
		return data[0], nil
	}
	_ = func() (byte, error) {
		defer DoWork()
		return rc(orig)
	}
}
