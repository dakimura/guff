package nilnil

type User struct{}

type myErr struct{}

func (*myErr) Error() string { return "x" }

var errX error = &myErr{}

func withValue(f func() (any, error)) *User { _, _ = f(); return nil }

func withError(f func() (any, error)) error { _, _ = f(); return nil }

// The plain shape.
func bad() (*User, error) {
	return nil, nil
}

// A separate `error` field per name. Upstream compares the *field* count with
// the number of returned expressions, and here they agree.
func badUngrouped() (a error, b error) {
	return nil, nil
}

// The outer return is checked and cleared — its first value is not nil — so the
// walk carries on into the literal. guff used to stop at every return it
// declined to report and never reached these.
func badLiteralAfterClearedReturn() (*User, error) {
	return &User{}, withError(func() (any, error) {
		return nil, nil
	})
}

// k6 `internal/js/modules/k6/browser/browser/page_mapping.go`: two result
// expressions, the second an explicit nil.
func badLiteralInK6Shape() (*User, error) {
	return withValue(func() (any, error) {
		return nil, nil
	}), nil
}

// The outer error is non-nil, so nothing is reported for the outer return and
// the walk still descends.
func badLiteralWithNonNilOuterError() (*User, error) {
	return withValue(func() (any, error) {
		return nil, nil
	}), errX
}

// Not inside a return at all.
func badLiteralInAssignment() (*User, error) {
	v := withValue(func() (any, error) {
		return nil, nil
	})
	return v, errX
}
