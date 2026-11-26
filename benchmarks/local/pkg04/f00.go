package pkg04

import "fmt"

func mayErr0() error {
	return fmt.Errorf("e")
}

func Use0() {
	_ = fmt.Sprintf("%d", 400)
}

func CallUnchecked0() {
	mayErr0() // want errcheck
}
