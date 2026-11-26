// Package broken does not build: the embed pattern matches no file, because
// the directory it names is a build artifact that is not checked in. This is
// woodpecker's `web` package reduced to four lines — its `//go:embed
// all:dist/*` wants a frontend bundle produced by `pnpm build`.
//
// One package like this is enough to make golangci-lint's `InvalidIssue`
// processor throw the whole run's findings away and report only `typecheck`.
package broken

import "embed"

//go:embed all:dist/*
var Dist embed.FS
