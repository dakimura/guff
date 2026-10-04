module example.com/gomoddirectives/ignore

go 1.25

// gomoddirectives v0.10 (golangci-lint 2.14.0) reports, whatever the config,
// an `ignore` element the go command already skips: vendor, testdata, or a
// name starting with '.' or '_' — and `..` starts with '.'.
ignore ./node_modules

ignore (
	./vendor/x
	_tmp
	.cache
	../outside
	"./testdata/sub"
	./docs/_build
)

// replace-allow-all short-circuits only the allow decision: the identical and
// the duplicate replace below are still reported.
replace example.com/y => example.com/z v1.2.3

replace example.com/y => example.com/z v1.2.3

replace example.com/same v1.0.0 => example.com/same v1.0.0
