package gosec_g115files

// SrcFuncs runs across files in the order they are given (go list: by name),
// so this file's `byte` names g115_files_b.go's `uint8`. Golden-only: the
// Rust fixture harness type-checks one file per package.

func Zeta(r byte) int8 { return int8(r) } // FINDING byte -> int8 (the first)
