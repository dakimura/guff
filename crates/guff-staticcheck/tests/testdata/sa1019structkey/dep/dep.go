package dep

type Result struct {
	// Deprecated: Use `RequeueAfter` instead.
	Requeue bool
	RequeueAfter int
}
