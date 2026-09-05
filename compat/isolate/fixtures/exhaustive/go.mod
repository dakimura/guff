module github.com/dakimura/guff/compat/isolate/fixtures/exhaustive

go 1.22

// A real second module, so its packages are not roots of the run: see
// enumdep/enum.go for why that matters.
require example.com/enumdep v0.0.0

replace example.com/enumdep => ./enumdep
