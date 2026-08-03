package old

// Deprecated: call New.
func Legacy() {}

// Deprecated: use NewClient instead.
type OldClient interface {
	Do()
}
