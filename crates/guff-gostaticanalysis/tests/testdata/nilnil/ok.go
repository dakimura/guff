package nilnil

type User struct{}

func ok() (*User, error) {
	return &User{}, nil
}

// A `return` the rule declines to judge takes its subtree with it: upstream's
// callback `return false`s on every path out of the ReturnStmt arm, and
// `inspector.Nodes` reads that as "do not descend". So the `nil, nil` inside a
// func literal that is *itself* returned is never visited. gitea writes six of
// these (`return db.WithTx2(ctx, func(…) (*Comment, error) { … })`).
func withTx[T any](f func() (T, error)) (T, error) { return f() }

func okReturnInsideReturnedLiteral() (*int, error) {
	return withTx(func() (*int, error) {
		return nil, nil
	})
}
