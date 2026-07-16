package example

import "context"

// OnlySingle has one unnamed parameter — flagged by default, skipped when
// skip-single-param is true.
type OnlySingle interface {
	Run(context.Context) error
}
