package contextcheck_closure

import "context"

type listFunc func(ns string) error

func doList(ctx context.Context, ns string) error {
	_ = ctx
	_ = ns
	return nil
}

// Capture `prefix` so SSA emits MakeClosure (matches helm RsListFromClient).
func fromClient(prefix string) listFunc {
	return func(ns string) error {
		return doList(context.Background(), prefix+ns)
	}
}

func getThings() error {
	return fromClient("p")("default")
}

func badCaller(ctx context.Context) {
	_ = getThings()
}
