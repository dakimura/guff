// The far end of a cross-package chain: a method that manufactures a context.
//
// Nothing here is a finding on its own — `Close` has no context to inherit. It
// is the *fact* this package exports about `(*Bulk).Close` that the importer
// two hops away needs, and a function the SSA builder creates on demand for an
// imported package carries no `Function.pkg` to look that fact up by.
package inner

import "context"

type Bulk struct{}

func work(ctx context.Context) error { _ = ctx; return nil }

func (b *Bulk) Close() error { return work(context.Background()) }
