package p

func init() {}

func Bad() {}

// Every `init` is its own finding, and the message is the same for each — so
// two of them in one file is the shape that shows the linter reports per
// declaration rather than per file.
func init() {}
