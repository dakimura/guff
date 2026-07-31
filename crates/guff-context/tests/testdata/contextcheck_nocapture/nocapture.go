package contextcheck_nocapture

import "context"

type listFunc func(ns string) error

func doList(ctx context.Context, ns string) error {
	_ = ctx
	_ = ns
	return nil
}

// Non-capturing func lit: SSA returns a bare Function (no MakeClosure).
// contextcheck must still chase that return into the closure body.
func fromClient() listFunc {
	return func(ns string) error {
		return doList(context.Background(), ns)
	}
}

func getThings() error {
	return fromClient()("default")
}

func badCaller(ctx context.Context) {
	_ = getThings()
}
