// Package sa1019stdlib uses deprecated stdlib API.
//
// SA1019 needs two things per finding: the version pair, which
// `knowledge.StdlibDeprecations` carries, and the deprecation's prose, which
// lives only in a GOROOT doc comment. A dependency read from export data has
// no doc comments, so guff reported nothing at all here until it learned to
// scan GOROOT sources for the packages that table names (2026-08-17 report,
// issue C). Fixture for compat/golden/cases/staticcheck-sa1019-stdlib.
//
// One of each shape the lookup discriminates on — package-level func, const,
// struct field, pointer method — because the field branch is the one that was
// broken and a `contains`-style fixture would not have told them apart.
package sa1019stdlib

import (
	"archive/zip"
	"crypto/ecdsa"
	"math/big"
	"net"
	"net/http"
	"path/filepath"
	"regexp"
)

// Package-level func, deprecated in go1.0.
func Prefix(a, b string) bool { return filepath.HasPrefix(a, b) }

// Struct field, deprecated in go1.10.
func Modified(h *zip.FileHeader) uint16 { return h.ModifiedTime }

// Struct field, deprecated in go1.7.
func Cancel(d *net.Dialer) <-chan struct{} { return d.Cancel }

// Pointer method, deprecated in go1.12.
func Copy(re *regexp.Regexp) *regexp.Regexp { return re.Copy() }

// Pointer method, deprecated in go1.6.
func CancelRequest(t *http.Transport, r *http.Request) { t.CancelRequest(r) }

// The report's own case: a struct field deprecated in go1.26.
func Coords(p *ecdsa.PublicKey) *big.Int { return p.X }
