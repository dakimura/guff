package p

import "errors"

var errSentinel = errors.New("x")

func Bad(err error) bool {
	return err == errSentinel
}

// The type-assertion check carries an `errors.As` suggested fix, in four
// shapes. The golden key stops at the message text so the fix bodies are
// invisible here — these pin the findings and their positions; the fix text is
// pinned by `errorlint_suggests_errors_as_for_type_assertions`.

type myErr struct{ msg string }

func (e *myErr) Error() string { return e.msg }

type valErr struct{ msg string }

func (e valErr) Error() string { return e.msg }

func AssignTwo(err error) string {
	target, ok := err.(*myErr)
	if !ok {
		return ""
	}
	return target.msg
}

func AssignBlank(err error) bool {
	_, ok := err.(*myErr)
	return ok
}

func IfInit(err error) string {
	if target, ok := err.(*myErr); ok {
		return target.msg
	}
	return ""
}

func AssignValue(err error) string {
	target, ok := err.(valErr)
	if !ok {
		return ""
	}
	return target.msg
}

func Standalone(err error) string {
	return err.(*myErr).msg
}
