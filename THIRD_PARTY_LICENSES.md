# Third-party licenses

guff is a Rust reimplementation / port of many Go analysis tools.
The **combined** `guff` binary is distributed under **GPL-3.0** (see [`LICENSE`](LICENSE)),
because it includes ports of GPL-3.0 upstream analyzers and links them into one executable.

This file lists the **original** licenses of the upstream projects that guff ports or
shells out to. Copyright remains with the respective upstream authors. Preserve these
notices when redistributing source.

License data was taken from GitHub’s license API (and LICENSE files where noted) on 2026-07-20.

## GPL-3.0 — analyzer ports (5)

These upstreams are ported into guff and linked into the `guff` binary:

- [OpenPeeDeeP/depguard](https://github.com/OpenPeeDeeP/depguard)
- [denis-tingaikin/go-header](https://github.com/denis-tingaikin/go-header)
- [firefart/nonamedreturns](https://github.com/firefart/nonamedreturns)
- [leonklingele/grouper](https://github.com/leonklingele/grouper)
- [xen0n/gosmopolitan](https://github.com/xen0n/gosmopolitan)

## GPL-3.0 — compatibility reference (not a line-for-line port)

- [golangci/golangci-lint](https://github.com/golangci/golangci-lint) — config / CLI / output compatibility target

## MPL-2.0 (1)

- [go-simpler/musttag](https://github.com/go-simpler/musttag)

## Apache-2.0 (14)

- [ClickHouse/clickhouse-go-linter](https://github.com/ClickHouse/clickhouse-go-linter)
- [ashanbrown/forbidigo](https://github.com/ashanbrown/forbidigo)
- [ashanbrown/makezero](https://github.com/ashanbrown/makezero)
- [charithe/durationcheck](https://github.com/charithe/durationcheck)
- [julz/importas](https://github.com/julz/importas)
- [ldez/exptostd](https://github.com/ldez/exptostd)
- [ldez/gomoddirectives](https://github.com/ldez/gomoddirectives)
- [ldez/tagliatelle](https://github.com/ldez/tagliatelle)
- [ldez/usetesting](https://github.com/ldez/usetesting)
- [manuelarte/embeddedstructfieldcheck](https://github.com/manuelarte/embeddedstructfieldcheck)
- [manuelarte/funcorder](https://github.com/manuelarte/funcorder)
- [securego/gosec](https://github.com/securego/gosec)
- [uudashr/iface](https://github.com/uudashr/iface)
- [yeya24/promlinter](https://github.com/yeya24/promlinter)

## MIT (71)

- [4meepo/tagalign](https://github.com/4meepo/tagalign)
- [Abirdcfly/dupword](https://github.com/Abirdcfly/dupword)
- [AdminBenni/iota-mixing](https://github.com/AdminBenni/iota-mixing)
- [AlwxSin/noinlineerr](https://github.com/AlwxSin/noinlineerr)
- [Antonboom/errname](https://github.com/Antonboom/errname)
- [Antonboom/nilnil](https://github.com/Antonboom/nilnil)
- [Antonboom/testifylint](https://github.com/Antonboom/testifylint)
- [Djarvur/go-err113](https://github.com/Djarvur/go-err113)
- [MirrexOne/unqueryvet](https://github.com/MirrexOne/unqueryvet)
- [alexkohler/dogsled](https://github.com/alexkohler/dogsled)
- [alexkohler/nakedret](https://github.com/alexkohler/nakedret)
- [alexkohler/prealloc](https://github.com/alexkohler/prealloc)
- [alingse/asasalint](https://github.com/alingse/asasalint)
- [bkielbasa/cyclop](https://github.com/bkielbasa/cyclop)
- [blizzy78/varnamelen](https://github.com/blizzy78/varnamelen)
- [bombsimon/wsl](https://github.com/bombsimon/wsl)
- [breml/bidichk](https://github.com/breml/bidichk)
- [breml/errchkjson](https://github.com/breml/errchkjson)
- [butuzov/ireturn](https://github.com/butuzov/ireturn)
- [butuzov/mirror](https://github.com/butuzov/mirror)
- [catenacyber/perfsprint](https://github.com/catenacyber/perfsprint)
- [ccojocar/zxcvbn-go](https://github.com/ccojocar/zxcvbn-go) — ported (entropy estimator behind gosec's G101), **including its frequency lists and adjacency graphs**, in `crates/guff-style/src/zxcvbn/`
- [ckaznocha/intrange](https://github.com/ckaznocha/intrange)
- [curioswitch/go-reassign](https://github.com/curioswitch/go-reassign)
- [dominikh/go-tools](https://github.com/dominikh/go-tools)
- [ghostiam/protogetter](https://github.com/ghostiam/protogetter)
- [go-critic/go-critic](https://github.com/go-critic/go-critic)
- [godoc-lint/godoc-lint](https://github.com/godoc-lint/godoc-lint)
- [golangci/asciicheck](https://github.com/golangci/asciicheck)
- [golangci/dupl](https://github.com/golangci/dupl)
- [golangci/go-printf-func-name](https://github.com/golangci/go-printf-func-name)
- [golangci/golines](https://github.com/golangci/golines)
- [golangci/misspell](https://github.com/golangci/misspell)
- [gordonklaus/ineffassign](https://github.com/gordonklaus/ineffassign)
- [gostaticanalysis/forcetypeassert](https://github.com/gostaticanalysis/forcetypeassert)
- [jgautheron/goconst](https://github.com/jgautheron/goconst)
- [jingyugao/rowserrcheck](https://github.com/jingyugao/rowserrcheck)
- [karamaru-alpha/copyloopvar](https://github.com/karamaru-alpha/copyloopvar)
- [kisielk/errcheck](https://github.com/kisielk/errcheck)
- [kulti/thelper](https://github.com/kulti/thelper)
- [kunwardeep/paralleltest](https://github.com/kunwardeep/paralleltest)
- [lasiar/canonicalheader](https://github.com/lasiar/canonicalheader)
- [leighmcculloch/gocheckcompilerdirectives](https://github.com/leighmcculloch/gocheckcompilerdirectives)
- [leighmcculloch/gochecknoglobals](https://github.com/leighmcculloch/gochecknoglobals)
- [leighmcculloch/gochecknoinits](https://github.com/leighmcculloch/gochecknoinits)
- [macabu/inamedparam](https://github.com/macabu/inamedparam)
- [maratori/testableexamples](https://github.com/maratori/testableexamples)
- [maratori/testpackage](https://github.com/maratori/testpackage)
- [matoous/godox](https://github.com/matoous/godox)
- [mgechev/revive](https://github.com/mgechev/revive)
- [moricho/tparallel](https://github.com/moricho/tparallel)
- [nunnatsa/ginkgolinter](https://github.com/nunnatsa/ginkgolinter)
- [polyfloyd/go-errorlint](https://github.com/polyfloyd/go-errorlint)
- [raeperd/recvcheck](https://github.com/raeperd/recvcheck)
- [ryancurrah/gomodguard](https://github.com/ryancurrah/gomodguard)
- [ryanrolds/sqlclosecheck](https://github.com/ryanrolds/sqlclosecheck)
- [sashamelentyev/interfacebloat](https://github.com/sashamelentyev/interfacebloat)
- [sashamelentyev/usestdlibvars](https://github.com/sashamelentyev/usestdlibvars)
- [sivchari/containedctx](https://github.com/sivchari/containedctx)
- [sonatard/noctx](https://github.com/sonatard/noctx)
- [ssgreg/nlreturn](https://github.com/ssgreg/nlreturn)
- [stbenjam/no-sprintf-host-port](https://github.com/stbenjam/no-sprintf-host-port)
- [tetafro/godot](https://github.com/tetafro/godot)
- [timakin/bodyclose](https://github.com/timakin/bodyclose)
- [timonwong/loggercheck](https://github.com/timonwong/loggercheck)
- [tomarrell/wrapcheck](https://github.com/tomarrell/wrapcheck)
- [tommy-muehle/go-mnd](https://github.com/tommy-muehle/go-mnd)
- [ultraware/funlen](https://github.com/ultraware/funlen)
- [ultraware/whitespace](https://github.com/ultraware/whitespace)
- [uudashr/gocognit](https://github.com/uudashr/gocognit)
- [yagipy/maintidx](https://github.com/yagipy/maintidx)

## BSD-3-Clause (7)

- [daixiang0/gci](https://github.com/daixiang0/gci)
- [fzipp/gocyclo](https://github.com/fzipp/gocyclo)
- [golang/go](https://github.com/golang/go)
- [golang/tools](https://github.com/golang/tools)
- [mdempsky/unconvert](https://github.com/mdempsky/unconvert)
- [mvdan/gofumpt](https://github.com/mvdan/gofumpt)
- [nishanths/predeclared](https://github.com/nishanths/predeclared)

## BSD-2-Clause (2)

- [nakabonne/nestif](https://github.com/nakabonne/nestif)
- [nishanths/exhaustive](https://github.com/nishanths/exhaustive)

## Unlicense (1)

- [alecthomas/go-check-sumtype](https://github.com/alecthomas/go-check-sumtype)

## Notes

- **GPL-3.0 ports** currently include: `OpenPeeDeeP/depguard`, `denis-tingaikin/go-header`,
  `firefart/nonamedreturns`, `leonklingele/grouper`, `xen0n/gosmopolitan`.
  Their presence in the linked binary requires the distributed work to be GPL-3.0.
- **golangci/golangci-lint** is GPL-3.0; guff aims for config/CLI compatibility and does not
  claim to be a line-for-line port of that repository.
- **MPL-2.0** (`go-simpler/musttag`): file-level copyleft; the ported logic remains under MPL-2.0 terms.
- `ashanbrown/forbidigo` and `ashanbrown/makezero` are listed as Apache-2.0 based on their LICENSE files
  (GitHub’s API reports `NOASSERTION`).
- Formatters (`gofmt` / `gofumpt` / `goimports` / `gci` / `golines` / `swaggo`) are typically
  invoked as external binaries; install and license those tools separately.
