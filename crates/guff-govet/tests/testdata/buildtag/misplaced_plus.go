package misplacedplus

// A +build comment is only meaningful in the file header. Upstream reports it
// wherever it appears afterwards; a //go:build comment in the same position is
// rejected by the compiler instead ("misplaced compiler directive"), so this
// file is the only misplacement shape a golden case can carry.

// +build linux

func f() {}
