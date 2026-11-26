package pkg

// AcrossFiles spreads its method set over the package's files, so the winner is
// not "first in the first file" either.
type AcrossFiles struct{}

func (z AcrossFiles) Zeta() {}

func (a AcrossFiles) Apex() {}
