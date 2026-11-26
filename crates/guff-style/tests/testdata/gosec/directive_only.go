// Package directiveonly carries gosec's *other* suppression spelling and
// nothing else.
//
// findNoSecDirective accepts two forms: the hash-prefixed tag anywhere in a
// comment group, and a comment that *starts with* the directive prefix
// (analyzer.go's `directivePrefix`). guff screened files with a byte search for
// the tag alone before reparsing them for comments — and the directive spelling
// does not contain it, so a file that uses only the directive form was never
// reparsed and every suppression in it was ignored. grafana/tempo's
// modules/livestore/live_store_background.go:270 is that file.
//
// Nothing in here may contain the five bytes of the tag, or the screen lets the
// file through for the wrong reason and the fixture stops testing anything.
//
// Each case is a separate function on purpose: a directive on a top-level
// `const` suppresses the whole file, because the comment attaches to a node
// whose range covers it (measured).
package directiveonly

func plain() string {
	token := "AZURE_STORAGE_KEY" // FINDING
	return token
}

func hushedNamed() string {
	token := "AZURE_STORAGE_KEY" //gosec:disable G101
	return token
}

func hushedEmpty() string {
	// An empty directive suppresses every rule.
	token := "AZURE_STORAGE_KEY" //gosec:disable
	return token
}

func hushedOther() string {
	// A directive naming a different rule leaves this one reported.
	token := "AZURE_STORAGE_KEY" //gosec:disable G102
	return token
}

func hushedWithReason() string {
	token := "AZURE_STORAGE_KEY" //gosec:disable G101 -- checked in on purpose
	return token
}
