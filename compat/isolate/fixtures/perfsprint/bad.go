package p

import (
	"errors"
	"fmt"
)

// perfsprint has a rule per conversion it can replace, and each names the
// replacement it wants — so each is its own sentence, not a repeat.

func StringFormat(s string) string {
	return fmt.Sprintf("%s", s)
}

func IntFormat(n int) string {
	return fmt.Sprintf("%d", n)
}

func ErrorNew(s string) error {
	return fmt.Errorf("%s", s)
}

func ErrorfNoArgs() error {
	return fmt.Errorf("constant message")
}

func SprintConcat(a, b string) string {
	return fmt.Sprint(a, b)
}

func BoolFormat(b bool) string {
	return fmt.Sprintf("%v", b)
}

func ErrorsNew() error {
	return errors.New(fmt.Sprintf("x"))
}
