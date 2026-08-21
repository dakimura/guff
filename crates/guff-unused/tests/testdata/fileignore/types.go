// nats-server writes exactly this at the top of `jetstream_helpers_test.go`,
// where `type cluster` is declared. The methods of that type are spread over
// the other `*_test.go` files, which carry no directive of their own.

//lint:file-ignore U1000 helpers shared between test files

package fileignore

// Upstream marks an ignored `*types.TypeName` used and then walks
// `typ.Methods()` — every method of the named type, whichever file it was
// written in (`unused/unused.go`, "use methods and fields of ignored types").
type cluster struct {
	n int
}

func (c *cluster) inIgnoredFile() int { return c.n }

// An ignored declaration is a *root*, not just a silenced report: what it
// references has to stay reachable.
func ignoredCaller() int { return keptAlive() }
