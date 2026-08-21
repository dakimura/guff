# guff

<p align="center">
  <a href="README.md">English</a> |
  <a href="README.zh-CN.md">简体中文</a> |
  <a href="README.es.md">Español</a> |
  <a href="README.pt-BR.md">Português (Brasil)</a> |
  <a href="README.ja.md">日本語</a>
</p>

<p align="center">
  <b>⚡ Un linter de Go ultrarrápido compatible con golangci-lint</b>
</p>

<p align="center">
  Ejecuta tus linters de Go en segundos, no en minutos.
</p>

<p align="center">
  <img src="assets/demo.gif" alt="golangci-lint termina en 22.1s; guff termina en 1.7s (helm, caché en frío)." width="820" />
</p>

<p align="center">
  <a href="docs/MIGRATION.md">Migrar en 5 minutos</a>
  ·
  <a href="docs/INSTALL.md">Instalar / desinstalar</a>
  ·
  <a href="docs/COMPARE.md">vs golangci-lint</a>
  ·
  <a href="docs/AGENTS.md">Agentes de IA</a>
</p>

---

## ¿Por qué guff?

`golangci-lint` es el agregador de linters estándar en Go — y es excelente.

Pero a medida que crecen los repositorios, el lint se convierte en una de las partes más lentas del ciclo de desarrollo.

Cada cambio local.  
Cada pull request.  
Cada iteración de un agente de codificación con IA.

Esperar importa.

**guff vuelve a hacer rápido el lint de Go.**

```
golangci-lint: 280s
guff:            20s

Mismo repositorio.
Misma configuración.
Mismos hallazgos.
```

---

## 🚀 Rendimiento

Repositorios open source reales con sus configuraciones existentes de `golangci-lint v2`:

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

Benchmarks en frío en Darwin arm64.

Resultados completos:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## ¿Por qué es rápido?

Los pipelines de lint tradicionales pagan una y otra vez el costo de:

- arrancar procesos
- cargar paquetes
- parsear el código fuente
- construir el estado de análisis

guff mantiene todo el pipeline de análisis dentro de **un solo proceso Rust**.

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

Un pipeline.  
Muchos analizadores.  
Menos espera.

---

## Compatibilidad drop-in con golangci-lint

¿Ya tienes un `.golangci.yml`?

Perfecto.

Consérvalo.

```bash
guff run ./...
```

guff lee automáticamente:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

Compatibilidad:

- ✅ 114 / 114 linters de golangci-lint v2 implementados
- ✅ Configuraciones existentes soportadas
- ✅ Múltiples formatos de salida
- ✅ Anotaciones de GitHub Actions

Comparación honesta (incluye lagunas parciales conocidas):

[`docs/COMPARE.md`](docs/COMPARE.md)

Matriz completa de compatibilidad:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

Migración en cinco minutos y rollback:

[`docs/MIGRATION.md`](docs/MIGRATION.md)

---

## Hecho para agentes de codificación con IA

Los agentes de IA ejecutan herramientas constantemente.

Un comando de lint lento se convierte en un ciclo de desarrollo lento.

guff está pensado para:

- Claude Code
- Cursor
- GitHub Copilot
- pipelines de CI
- desarrollo local

Instrucciones listas para copiar: [`docs/AGENTS.md`](docs/AGENTS.md) — pégalas en `CLAUDE.md` para Claude Code, en `.cursor/rules` para Cursor, o en el system prompt de tu agente.

---

## Pruébalo ahora

### Instalar (no hace falta Rust)

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh
```

Se instala en `~/.local/bin` por defecto. Luego:

```bash
guff run ./...
```

Eso es todo. Tu `.golangci.yml` existente funciona.

Otros instaladores:

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

Las configs no se tocan — puedes volver a golangci-lint en CI cuando quieras. Detalles: [`docs/INSTALL.md`](docs/INSTALL.md#uninstall--rollback)

---

## Comandos habituales

```bash
# Ejecutar los linters configurados
guff run ./...

# Mostrar linters habilitados
guff linters

# Usar el preset fast
guff run --preset fast ./...

# Habilitar linters adicionales
guff run \
  --enable revive \
  --enable misspell \
  ./...

# Aplicar correcciones sugeridas
guff run --fix ./...

# Formateadores (gofmt / goimports / … según config)
guff fmt .

# Re-lint al cambiar (mantiene el análisis en caliente)
guff run --watch ./...

# Caché de issues
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

La Action conserva la caché de análisis de guff entre ejecuciones, de modo que un
pull request vuelve a analizar solo lo que cambió. Medido en un runner de GitHub
sobre un módulo de 113 paquetes: 7,9 s en frío, 0,2 s sin cambios y 4,2 s tras
editar un archivo con muchos dependientes. Matrices, tamaño de la caché y runners
autoalojados en [`docs/CI.md`](docs/CI.md).

---

## Docker

guff necesita un toolchain de Go para resolver paquetes.

La imagen oficial de Docker ya incluye Go.

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  ghcr.io/dakimura/guff:0.6.0 \
  run ./...
```

Opcional: persistir cachés de Go entre ejecuciones:

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

# Configuración

guff admite los archivos de configuración existentes de `golangci-lint`.

Orden de búsqueda:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

Ejemplo:

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

Ejecutar con:

```bash
guff run .
```

o indicar un config:

```bash
guff run -c .golangci.yml .
```

v1 → v2: `guff migrate`

---

# Linters soportados

guff implementa el conjunto completo de linters de golangci-lint v2.

Compatibilidad actual:

```
114 / 114 linters supported
```

Ejemplos:

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

Habilitar linters adicionales:

```bash
guff run \
  --enable revive \
  --enable gosec \
  ./...
```

Matriz completa:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

---

# Formatos de salida

Formatos soportados:

- text
- colored-line-number
- json
- checkstyle
- sarif
- tab
- colored-tab
- github-actions

Ejemplo:

```bash
guff run \
  --out-format github-actions \
  ./...
```

GitHub Actions anotará automáticamente los pull requests.

---

# Arquitectura

guff se construye alrededor de un pipeline de análisis compartido.

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

A diferencia de los agregadores tradicionales, guff evita reconstruir el estado de análisis para cada herramienta.

El resultado:

- menos overhead de arranque
- menor uso de memoria
- bucles de feedback más rápidos

---

# Desarrollo

Requisitos:

- Go
- Rust (edition 2021)

Compilar:

```bash
cargo build
```

Probar:

```bash
cargo test
```

Ejecutar en local:

```bash
cargo run -p guff-lint -- run ./...
```

---

## Benchmarks

Compilar el binario release:

```bash
cargo build --release -p guff-lint
```

Ejecutar benchmarks:

```bash
./benchmarks/smoke.sh

./benchmarks/run.sh
```

Benchmarks de repositorios OSS:

```bash
./benchmarks/run.sh \
  --oss \
  --tier pr,nightly,weekly
```

Resultados:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## Pruebas de compatibilidad

guff compara de forma continua los hallazgos con golangci-lint.

```bash
./compat/run.sh \
  --oss \
  --tier pr
```

Aislamiento por linter (uno habilitado a la vez):

```bash
./compat/run.sh --isolate --smoke
./compat/run.sh --isolate
```

El objetivo:

> Misma config. Mismos hallazgos. Ejecución mucho más rápida.

---

## Puerta de regresión de Prometheus

guff incluye una suite de regresión contra Prometheus.

Comprueba:

- tiempo de ejecución
- memoria peak RSS
- diferencias de hallazgos

```bash
./regress/run.sh
```

Perfil completo:

```bash
./regress/run.sh \
  --profile full
```

---

# Estructura del código

Workspace de Cargo:

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

Componentes principales:

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

# Licencia

GPL-3.0

Usar el CLI `guff` en CI o en local **no** GPL-iza tu aplicación Go. Detalles: [`docs/LICENSE-FAQ.md`](docs/LICENSE-FAQ.md)

Verificación de releases / SBOM: [`docs/SUPPLY-CHAIN.md`](docs/SUPPLY-CHAIN.md)

guff incluye ports y adaptaciones de analizadores de varios proyectos Go upstream.

Consulta:

- [`LICENSE`](LICENSE)
- [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)
