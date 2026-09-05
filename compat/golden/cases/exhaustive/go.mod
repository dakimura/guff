module github.com/dakimura/guff/compat/isolate/fixtures/exhaustive

go 1.22

// The enum-declaring package lives in a module of its own so the run never
// analyses it — see enumdep/enum.go.
require example.com/enumdep v0.0.0

replace example.com/enumdep => ./enumdep
