package ok

func withRecover() (reterr error) {
	defer func() {
		if r := recover(); r != nil {
			reterr = nil
		}
	}()
	panic("x")
}
