package p

func do() error  { return nil }
func do2() error { return nil }

func Bad() error {
	err := do()
	if err != nil {
		return err
	}
	err2 := do2()
	if err2 != nil {
		return err // wrong error
	}
	return nil
}

// nilnesserr names the call, so a second wrong-error return reads the same but
// sits at a different position — and a variadic call is the other arm.
func AlsoBad() error {
	err := do()
	if err != nil {
		return err
	}

	err3 := do2()
	if err3 != nil {
		return err
	}

	return nil
}

// The variadic arm, which needs the widening peeled: `%v` takes an `any`, so
// the value the check sees is the error inside a `ChangeInterface`. A version
// that does not read through it says nothing here while still reporting
// `sink(err)` below, which needs no widening at all.
//
// syncthing `cmd/strelaysrv/pool.go` is this shape: the inner `err :=` shadows
// the outer one, and the message names the outer error, which is nil.
func VariadicBad() {
	err := do()
	if err != nil {
		return
	}

	if err := do2(); err == nil {
		return
	}

	logf("failed: %v", err)
}

// The same without the shadowing, and with two separate variables.
func VariadicTwoVars() {
	err1 := do()
	if err1 != nil {
		return
	}

	err2 := do2()
	if err2 == nil {
		return
	}

	logf("failed: %v", err1)
}

// The non-variadic arm: no widening happens, so this one always reported.
func NonVariadicBad() {
	err1 := do()
	if err1 != nil {
		return
	}

	err2 := do2()
	if err2 == nil {
		return
	}

	sink(err1)
}

// silent — the error the message names is the one that was checked.
func SameError() {
	err := do()
	if err != nil {
		return
	}

	logf("failed: %v", err)
}

func logf(format string, args ...any) {}

func sink(err error) {}
