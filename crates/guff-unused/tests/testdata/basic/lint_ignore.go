package lintignore

// honnef's own suppression syntax: an object on a `//lint:ignore U1000` line is
// considered used. rclone's `cmd/serve/docker` writes one of each spelling — a
// trailing comment on a `var`, and a doc comment on a `func` that only the
// linux build calls.
var (
	ignoredVar   = "/run/docker/plugins" //lint:ignore U1000 unused when not building linux
	reportedVar  = "/etc/docker/plugins"
	anotherCheck = "x" //lint:ignore SA4006 a different check does not silence U1000
)

//lint:ignore U1000 unused when not building linux
func ignoredFunc() string {
	return ""
}

func reportedFunc() string {
	return ""
}

func Run() string { return ignoredVar + anotherCheck }
