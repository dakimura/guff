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

// `err-error` puts the rendered value in the *message*
// (`fn+" can be replaced with "+errMethodCall`), so a value the two renderers
// spell differently is what makes the rendering verifiable. perfsprint uses
// `format.Node`, which drops the blanks around a higher-precedence operator.
type wrapper struct{ errs []error }

func ErrErrorIndexed(w wrapper, i, j int) string {
	return fmt.Sprintf("%v", w.errs[i*2+j])
}

func ErrErrorCalled(f func() error) string {
	return fmt.Sprintf("%s", f())
}
