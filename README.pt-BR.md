# guff

<p align="center">
  <a href="README.md">English</a> |
  <a href="README.zh-CN.md">简体中文</a> |
  <a href="README.es.md">Español</a> |
  <a href="README.pt-BR.md">Português (Brasil)</a> |
  <a href="README.ja.md">日本語</a> |
  <a href="README.hi.md">हिन्दी</a>
</p>

<p align="center">
  <b>⚡ Um linter de Go ultrarrápido compatível com golangci-lint</b>
</p>

<p align="center">
  Rode seus linters de Go em segundos, não em minutos.
</p>

<p align="center">
  <img src="assets/demo.gif" alt="golangci-lint termina em 22.1s; guff termina em 1.7s (helm, cache frio)." width="820" />
</p>

<p align="center">
  <a href="docs/MIGRATION.md">Migrar em 5 minutos</a>
  ·
  <a href="docs/INSTALL.md">Instalar / desinstalar</a>
  ·
  <a href="docs/COMPARE.md">vs golangci-lint</a>
  ·
  <a href="docs/AGENTS.md">Agentes de IA</a>
</p>

---

## Por que guff?

`golangci-lint` é o agregador padrão de linters em Go — e é excelente.

Mas à medida que os repositórios crescem, o lint se torna uma das partes mais lentas do ciclo de desenvolvimento.

Cada mudança local.  
Cada pull request.  
Cada iteração de um agente de codificação com IA.

Esperar importa.

**guff deixa o lint de Go rápido de novo.**

```
golangci-lint: 280s
guff:            20s

Mesmo repositório.
Mesma configuração.
Mesmos achados.
```

---

## 🚀 Desempenho

Repositórios open source reais com suas configurações existentes de `golangci-lint v2`:

| Repository | golangci-lint | guff | Speedup |
|---|---:|---:|---:|
| grafana | 279.8s | **19.8s** | **14× faster** |
| consul | 38.0s | **5.2s** | **7× faster** |
| helm | 17.5s | **1.4s** | **13× faster** |
| k9s | 14.6s | **2.2s** | **7× faster** |
| caddy | 9.1s | **0.85s** | **11× faster** |
| containerd | 5.2s | **0.37s** | **14× faster** |
| gin | 3.9s | **0.38s** | **10× faster** |
| cobra | 1.4s | **0.23s** | **6× faster** |

Benchmarks em cache frio no Darwin arm64.

Resultados completos:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## Por que é rápido?

Pipelines de lint tradicionais pagam repetidamente o custo de:

- iniciar processos
- carregar pacotes
- fazer parse do código-fonte
- construir o estado de análise

guff mantém todo o pipeline de análise dentro de **um único processo Rust**.

```
Go source
   |
   v
Package loading
   |
   v
Type checking
   |
   v
Shared analysis pipeline
   |
   v
All linters
```

Um pipeline.  
Muitos analisadores.  
Menos espera.

---

## Compatibilidade drop-in com golangci-lint

Já tem um `.golangci.yml`?

Ótimo.

Mantenha.

```bash
guff run ./...
```

guff lê automaticamente:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

Compatibilidade:

- ✅ 114 / 114 linters do golangci-lint v2 implementados
- ✅ Configurações existentes suportadas
- ✅ Vários formatos de saída
- ✅ Anotações do GitHub Actions

Comparação honesta (incluindo lacunas parciais conhecidas):

[`docs/COMPARE.md`](docs/COMPARE.md)

Matriz completa de compatibilidade:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

Migração em cinco minutos e rollback:

[`docs/MIGRATION.md`](docs/MIGRATION.md)

---

## Feito para agentes de codificação com IA

Agentes de IA executam ferramentas o tempo todo.

Um comando de lint lento vira um ciclo de desenvolvimento lento.

guff foi pensado para:

- Claude Code
- Cursor
- GitHub Copilot
- pipelines de CI
- desenvolvimento local

Instruções prontas para colar: [`docs/AGENTS.md`](docs/AGENTS.md) — cole no `CLAUDE.md` para o Claude Code, no `.cursor/rules` para o Cursor, ou no system prompt do seu agente.

---

## Experimente agora

### Instalar (não precisa de Rust)

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh
```

Instala em `~/.local/bin` por padrão. Depois:

```bash
guff run ./...
```

É isso. Seu `.golangci.yml` existente funciona.

Outros instaladores:

```bash
# Homebrew
brew tap dakimura/guff https://github.com/dakimura/guff
brew install guff
```

Docker, aqua, Actions, cargo: [`docs/INSTALL.md`](docs/INSTALL.md)

### Desinstalar / rollback

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/uninstall.sh | sh
```

As configs não são alteradas — você pode voltar o CI para golangci-lint a qualquer momento. Detalhes: [`docs/INSTALL.md`](docs/INSTALL.md#uninstall--rollback)

---

## Comandos comuns

```bash
# Rodar os linters configurados
guff run ./...

# Mostrar linters habilitados
guff linters

# Usar o preset fast
guff run --preset fast ./...

# Habilitar linters adicionais
guff run \
  --enable revive \
  --enable misspell \
  ./...

# Aplicar correções sugeridas
guff run --fix ./...

# Formatadores (gofmt / goimports / … da config)
guff fmt .

# Re-lint ao mudar (mantém a análise aquecida)
guff run --watch ./...

# Cache de issues
guff cache status
guff cache clean
```

Editores, pre-commit, lefthook: [`docs/EDITORS.md`](docs/EDITORS.md)

---

## GitHub Actions

```yaml
name: lint

on:
  pull_request:

jobs:
  guff:
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-go@v5
        with:
          go-version: stable

      - uses: dakimura/guff@v0.6.0
        with:
          args: run --out-format=github-actions ./...
```

A Action preserva o cache de análise do guff entre execuções, então um pull
request analisa novamente apenas o que mudou. Medido em um runner do GitHub sobre
um módulo de 113 pacotes: 7,9 s a frio, 0,2 s sem alterações e 4,2 s após editar
um arquivo com muitos dependentes. Matrizes, tamanho do cache e runners
auto-hospedados em [`docs/CI.md`](docs/CI.md).

---

## Docker

guff precisa de um toolchain Go para resolver pacotes.

A imagem oficial Docker já inclui Go.

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  ghcr.io/dakimura/guff:0.6.0 \
  run ./...
```

Opcional: persistir caches Go entre execuções:

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  -v "$(go env GOMODCACHE)":/go/pkg/mod \
  -v "$(go env GOCACHE)":/root/.cache/go-build \
  -e GOMODCACHE=/go/pkg/mod \
  -e GOCACHE=/root/.cache/go-build \
  ghcr.io/dakimura/guff:0.6.0 \
  run ./...
```

---

# Configuração

guff oferece suporte aos arquivos de configuração existentes do `golangci-lint`.

Ordem de busca:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

Exemplo:

```yaml
version: "2"

linters:
  default: standard

  enable:
    - revive
    - misspell

  disable:
    - unused

  settings:
    errcheck:
      check-blank: true

formatters:
  enable:
    - gofmt
    - goimports
```

Rodar com:

```bash
guff run .
```

ou indicar um config:

```bash
guff run -c .golangci.yml .
```

v1 → v2: `guff migrate`

---

# Linters suportados

guff implementa o conjunto completo de linters do golangci-lint v2.

Compatibilidade atual:

```
114 / 114 linters supported
```

Exemplos:

| Linter | Description |
|---|---|
| staticcheck | Static analysis suite |
| govet | Go vet analyzers |
| errcheck | Unchecked errors |
| ineffassign | Ineffectual assignments |
| unused | Unused declarations |
| revive | Go style checker |
| gosec | Security checks |
| misspell | Spelling mistakes |
| gocritic | Code quality checks |
| dupl | Duplicate code detection |

Habilitar linters adicionais:

```bash
guff run \
  --enable revive \
  --enable gosec \
  ./...
```

Matriz completa:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

---

# Formatos de saída

Formatos suportados:

- text
- colored-line-number
- json
- checkstyle
- sarif
- tab
- colored-tab
- github-actions

Exemplo:

```bash
guff run \
  --out-format github-actions \
  ./...
```

O GitHub Actions anotará automaticamente os pull requests.

---

# Arquitetura

guff é construído em torno de um pipeline de análise compartilhado.

```
go list
  |
  v
Package loading
  |
  v
Type checking
  |
  v
Analysis passes
  |
  v
Dependency-aware execution graph
  |
  v
Diagnostics
```

Diferente dos agregadores tradicionais, guff evita reconstruir o estado de análise para cada ferramenta.

O resultado:

- menos overhead de inicialização
- menor uso de memória
- ciclos de feedback mais rápidos

---

# Desenvolvimento

Requisitos:

- Go
- Rust (edition 2021)

Build:

```bash
cargo build
```

Testes:

```bash
cargo test
```

Rodar localmente:

```bash
cargo run -p guff-lint -- run ./...
```

---

## Benchmarks

Build do binário release:

```bash
cargo build --release -p guff-lint
```

Rodar benchmarks:

```bash
./benchmarks/smoke.sh

./benchmarks/run.sh
```

Benchmarks de repositórios OSS:

```bash
./benchmarks/run.sh \
  --oss \
  --tier pr,nightly,weekly
```

Resultados:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## Testes de compatibilidade

guff compara continuamente os achados com o golangci-lint.

```bash
./compat/run.sh \
  --oss \
  --tier pr
```

Isolamento por linter (um habilitado por vez):

```bash
./compat/run.sh --isolate --smoke
./compat/run.sh --isolate
```

O objetivo:

> Mesma config. Mesmos achados. Execução bem mais rápida.

---

## Gate de regressão do Prometheus

guff inclui uma suíte de regressão contra o Prometheus.

Verifica:

- tempo de execução
- memória peak RSS
- diferenças de achados

```bash
./regress/run.sh
```

Perfil completo:

```bash
./regress/run.sh \
  --profile full
```

---

# Layout do código

Workspace Cargo:

```
guff/
├── crates/
│   ├── guff-lint/
│   ├── guff-runner/
│   ├── guff-analysis/
│   ├── guff-packages/
│   ├── guff-types/
│   ├── guff-ast/
│   ├── guff-ssa/
│   └── ...
│
├── benchmarks/
├── compat/
├── regress/
├── packaging/          # aqua registry draft
├── Formula/            # Homebrew tap formula
└── docs/
```

Componentes principais:

| Layer | Responsibility |
|---|---|
| CLI | Config, commands, output |
| Runner | Parallel analyzer execution |
| Analysis | Shared analysis framework |
| Packages | Go package loading |
| Types | Type checking |
| SSA | Go SSA implementation |
| AST | Go parser/token support |

---

# Licença

GPL-3.0

Usar o CLI `guff` em CI ou localmente **não** torna sua aplicação Go GPL. Detalhes: [`docs/LICENSE-FAQ.md`](docs/LICENSE-FAQ.md)

Verificação de releases / SBOM: [`docs/SUPPLY-CHAIN.md`](docs/SUPPLY-CHAIN.md)

guff inclui ports e adaptações de analisadores de vários projetos Go upstream.

Veja:

- [`LICENSE`](LICENSE)
- [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)
