package contextcheck_nocapture

import "context"

type listFunc func(ns string) error

func doList(ctx context.Context, ns string) error {
	_ = ctx
	_ = ns
	return nil
}

// Non-capturing func lit: SSA returns a bare Function (no MakeClosure), and
// upstream's `getCtxType` answers only for calls and closures — so this return
// is not followed and neither tool reports anything in this file. The capturing
// twin next door (contextcheck_closure) is the one that is a finding.
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
