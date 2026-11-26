// The module's go line is "1.22.0": the type checker's package version keeps
// the patch, the file version is its Lang, and upstream says "file is" when
// the two strings differ.
package stdversion

import (
	"crypto/mlkem"
	"crypto/tls"
	"log/slog"
	"os"
	"slices"
	"sync"
)

func shapes(s []int) {
	// A generic function: Uses records the instance, origin() the declaration.
	_ = slices.Collect(slices.Values(s))

	// A package-level var and const.
	_ = slog.DiscardHandler
	_ = mlkem.SeedSize

	// A new field of an older type.
	_ = tls.Config{}.EncryptedClientHelloKeys

	// A new type, and a method of it: only the type is reported.
	r, _ := os.OpenRoot(".")
	_ = r.Close()

	// A method reached through an alias of the declaring type.
	type wg = sync.WaitGroup
	var g wg
	g.Go(func() {})
}
