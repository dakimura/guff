package dotimport

import (
	"fmt"

	. "example.com/dotimport/lib"
)

func Two() string {
	// A findings anchor that only a type-aware analyzer produces: if this
	// package goes ill-typed, this line goes quiet on guff's side and the
	// golden no longer matches.
	fmt.Printf("%d", Farewell())
	return Farewell()
}

func Three() error {
	if ErrGone != nil {
		return ErrGone
	}
	return nil
}

func doWork() error { return nil }

// Four compares against a dot-imported sentinel — errorlint needs the type of
// both sides, so this line is a second type-aware anchor.
func Four() bool {
	err := doWork()
	return err == ErrGone
}
