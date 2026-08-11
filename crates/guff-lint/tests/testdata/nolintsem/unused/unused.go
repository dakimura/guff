// Package unused is about the *unused* candidate, which is the one nolintlint
// finding the filter has to settle.
//
// The linter names in these directives are the ones nolintlint parsed, spelled
// exactly as written: they are not lowercased and aliases are not resolved.
// The filter then drops any candidate whose name is not an enabled linter, so
// a misspelled or aliased name silently produces nothing at all.
package unused

func Specific() {
	//nolint:errcheck // reported: errcheck is enabled and suppressed nothing
	_ = 1
}

func WrongCase() {
	//nolint:ErrCheck // not reported: no enabled linter is named `ErrCheck`
	_ = 1
}

func Alias() {
	//nolint:megacheck // not reported: the alias resolves for the filter, but
	// the candidate is emitted under the name `megacheck`
	_ = 1
}

func Disabled() {
	//nolint:bodyclose // not reported: bodyclose is not enabled here
	_ = 1
}

func Bare() {
	//nolint // reported without a linter name
	_ = 1
}

func All() {
	//nolint:all // reported without a linter name, like the bare form
	_ = 1
}

func Malformed() {
	//nolint :errcheck // reported as malformed only — no unused candidate
	_ = 1
}
