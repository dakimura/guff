package errorlint

type myErr struct{ msg string }

func (e *myErr) Error() string { return e.msg }

type valErr struct{ msg string }

func (e valErr) Error() string { return e.msg }

// Two-value assignment: the variable keeps its name.
func AssignTwo(err error) string {
	target, ok := err.(*myErr)
	if !ok {
		return ""
	}
	return target.msg
}

// The second variable's name is preserved rather than forced to "ok".
func AssignNamedOk(err error) string {
	e, wasFound := err.(*myErr)
	if !wasFound {
		return ""
	}
	return e.msg
}

// `_` means the name is invented from the type, lower-cased.
func AssignBlank(err error) bool {
	_, ok := err.(*myErr)
	return ok
}

// The assignment is the if statement's initializer, so the whole head of the
// if is replaced.
func IfInit(err error) string {
	if target, ok := err.(*myErr); ok {
		return target.msg
	}
	return ""
}

// A non-pointer target declares instead of composing.
func AssignValue(err error) string {
	target, ok := err.(valErr)
	if !ok {
		return ""
	}
	return target.msg
}

// Standalone: wrapped in an immediately-called function literal. No import,
// because this fixture is type-checked with only the stubs in its own
// directory.
func Standalone(err error) string {
	return err.(*myErr).msg
}
