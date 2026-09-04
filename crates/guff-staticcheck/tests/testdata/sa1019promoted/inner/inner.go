// Package inner is imported by dep, and by nothing the use site imports.
package inner

// Deep is embedded by dep.Mid, which is embedded by dep.Outer, so the use site
// reaches DeepOld without ever naming this package.
type Deep struct {
	// Deprecated: deep field message.
	DeepOld string
	Fine    string
}
