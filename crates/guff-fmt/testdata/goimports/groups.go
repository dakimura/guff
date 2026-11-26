package groups

// The `local-prefixes` group has no blank line before it, and the two paths are
// already in sorted order — so the whole fix is one inserted line between two
// original ones. golangci-lint anchors an addition with no deletion in front of
// it at the *last original line*
// (`pkg/goformatters/internal/diff.go`'s `handleAddedOnlyLines`), so the
// finding lands on the third-party import above, not on the local import below.
// beats' `x-pack/osquerybeat/internal/install/artifact/artifact_test.go` is
// this shape, with `github.com/Masterminds/semver` above `github.com/elastic/…`.

import (
	"fmt"
	"os"

	"third.example/pkg"
	"zlocal.example/local"
)

var _ = fmt.Sprint
var _ = os.Getenv
var _ = pkg.Name
var _ = local.Name
