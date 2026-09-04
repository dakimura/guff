package nilnil

type User struct{}

func ok() (*User, error) {
	return &User{}, nil
}

func withTx[T any](f func() (T, error)) (T, error) { return f() }

// gitea `return db.WithTx2(ctx, func(…) (*Comment, error) { … })`: the outer
// return has a single result expression, so upstream stops there and never
// looks inside. gitea writes six of these and golangci-lint reports none.
func okReturnInsideReturnedLiteral() (*int, error) {
	return withTx(func() (*int, error) {
		return nil, nil
	})
}

// Two levels of the same thing.
func okNestedTwice() (*int, error) {
	return withTx(func() (*int, error) {
		return withTx(func() (*int, error) {
			return nil, nil
		})
	})
}

// One field holding two names is *one* entry in `ft.Results.List`, against two
// returned expressions — upstream drops the return before checking anything.
// Spelling the same signature out (see `badUngrouped`) is reported.
func okGrouped() (a, b error) {
	return nil, nil
}

func okGroupedThree() (a, b *User, err error) {
	return nil, nil, nil
}

// `only-two` pins the error slot to index 1, and `*User` does not implement
// error, so this is dropped rather than checked against index 2.
func okThreeResults() (*User, *User, error) {
	return nil, nil, nil
}
