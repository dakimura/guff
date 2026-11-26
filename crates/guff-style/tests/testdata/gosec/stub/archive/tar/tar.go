// Package tar stands in for `archive/tar`. Only the type identity matters:
// `argTypes` is compared against `types.Type.String()`, which spells the
// package *path*, so this has to sit at `archive/tar`.
package tar

import "os"

type Header struct {
	Name     string
	Linkname string
}

func (h *Header) FileInfo() os.FileInfo { return nil }
