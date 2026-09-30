// Package oldinner declares a deprecated method that `oldmethod.Wrap` promotes.
// The use site never imports this package: the declaring package is reached
// only through the embedding.
package oldinner

type Core struct{}

// Old is promoted onto oldmethod.Wrap.
//
// Deprecated: inner.
func (*Core) Old() {}
