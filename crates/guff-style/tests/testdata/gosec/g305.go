// G305 — file traversal when extracting a zip or tar archive.
//
// The whole rule is: `filepath.Join`/`path.Join` with an argument that came
// off a `*archive/zip.File` or a `*archive/tar.Header`.
//
//	if baseType := getArchiveBaseType(arg, ctx, file); baseType != nil {
//	    if slices.Contains(a.argTypes, baseType.String()) { … }
//	}
//
// There is no attempt to see whether the joined path is checked afterwards,
// which is why every real extractor carries a `//nolint:gosec` over the join.
// beats has three of those, and while this rule was missing they looked like
// *unused* directives — so nolintlint reported them instead, one guff-only row
// each.
//
// `getArchiveBaseType` reaches the type two ways: straight off a selector, or
// through the `:=` that declared a local from one. The second is what makes
// the rule work on real extractors, which read `header.Name` into a variable
// before joining it.
package gosec

import (
	"archive/tar"
	"archive/zip"
	"path"
	"path/filepath"
)

// --- reported ---

func g305TarDirect(header *tar.Header, dest string) string {
	return filepath.Join(dest, header.Name)
}

func g305ZipDirect(file *zip.File, dest string) string {
	return filepath.Join(dest, file.Name)
}

// Through the `:=` that declared it.
func g305ViaLocal(header *tar.Header, dest string) string {
	name := header.Name
	return filepath.Join(dest, name)
}

// `path.Join` is the second entry of the call list.
func g305PathJoin(file *zip.File, dest string) string {
	return path.Join(dest, file.Name)
}

// Two joins, two findings — the second one's *other* argument is a nested
// call, which answers nothing, but `header.Linkname` still matches.
func g305Nested(header *tar.Header, dest string) string {
	p := filepath.Join(dest, header.Name)
	return filepath.Join(filepath.Dir(p), header.Linkname)
}

// --- silent ---

// A value receiver: `tar.Header` is not `*tar.Header`.
func g305ValueHeader(header tar.Header, dest string) string {
	return filepath.Join(dest, header.Name)
}

type g305Other struct{ Name string }

func g305Unrelated(o *g305Other, dest string) string {
	return filepath.Join(dest, o.Name)
}

// The local was declared from a call, not a selector.
func g305LocalFromCall(header *tar.Header, dest string) string {
	name := header.FileInfo().Name()
	return filepath.Join(dest, name)
}

// Not a `Join`.
func g305NotJoin(header *tar.Header) string {
	return filepath.Clean(header.Name)
}
