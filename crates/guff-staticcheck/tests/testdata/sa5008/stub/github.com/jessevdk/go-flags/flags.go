package flags

// Enough of go-flags for the SA5008 duplicate-tag exemption: only the *import
// path* matters — upstream reads the file's imports, not the package's API.

type Options int

const Default Options = 1
