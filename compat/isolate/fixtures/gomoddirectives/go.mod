module github.com/dakimura/guff/compat/isolate/fixtures/gomoddirectives

go 1.22

// A local replace and a module replace are two different sentences upstream
// ("local replacement are not allowed" vs "replacement are not allowed"), and
// retract / exclude / toolchain each render as "<directive>: <reason>".
replace example.com/x => ../x

replace example.com/y => example.com/z v1.2.3

exclude example.com/w v1.0.0

retract v0.0.1

toolchain go1.24.0
