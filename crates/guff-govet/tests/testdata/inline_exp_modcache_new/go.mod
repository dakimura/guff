module example.com/govet/inline_exp_modcache_new

go 1.24

require golang.org/x/exp v0.0.0-20230626212559-97b1e661b5df

// A filesystem replace, so `go list -f {{.Dir}} -- golang.org/x/exp/maps`
// resolves with no network and no module download. What is being tested is
// that guff asks `go list` at all when there is no vendor directory, not how
// the module cache is populated.
replace golang.org/x/exp => ./exp
