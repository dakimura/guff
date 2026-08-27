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
  <b>⚡ golangci-lint के साथ संगत, बेहद तेज़ Go लिंटर</b>
</p>

<p align="center">
  अपने Go लिंटर मिनटों में नहीं, सेकंडों में चलाइए।
</p>

<p align="center">
  <img src="assets/demo.gif" alt="golangci-lint run 22.1s में पूरा होता है; guff run 1.7s में (helm, कोल्ड कैश)।" width="820" />
</p>

<p align="center">
  <a href="docs/MIGRATION.md">5 मिनट में माइग्रेट करें</a>
  ·
  <a href="docs/INSTALL.md">इंस्टॉल / अनइंस्टॉल</a>
  ·
  <a href="docs/COMPARE.md">golangci-lint से तुलना</a>
  ·
  <a href="docs/AGENTS.md">AI एजेंट</a>
</p>

---

## guff क्यों?

`golangci-lint` Go की मानक लिंटर एग्रीगेटर है — और यह बेहतरीन है।

लेकिन जैसे-जैसे Go रिपॉज़िटरी बड़ी होती जाती है, लिंटिंग डेवलपमेंट लूप के सबसे धीमे हिस्सों में से एक बन जाती है।

हर लोकल बदलाव पर।  
हर पुल रिक्वेस्ट पर।  
हर AI कोडिंग एजेंट के हर चक्र पर।

इंतज़ार मायने रखता है।

**guff, Go लिंटिंग को फिर से तेज़ बनाता है।**

```
golangci-lint: 280s
guff:            20s

Same repository.
Same config.
Same findings.
```

---

## 🚀 परफ़ॉर्मेंस

असली ओपन-सोर्स रिपॉज़िटरी, उनके मौजूदा `golangci-lint v2` कॉन्फ़िगरेशन के साथ:

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

कोल्ड-कैश बेंचमार्क, Darwin arm64 पर।

पूरे बेंचमार्क परिणाम:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## guff तेज़ क्यों है?

पारंपरिक लिंट पाइपलाइन बार-बार यह कीमत चुकाती हैं:

- प्रोसेस शुरू करना
- पैकेज लोड करना
- सोर्स कोड पार्स करना
- विश्लेषण की स्थिति (analysis state) बनाना

guff पूरी विश्लेषण पाइपलाइन को एक ही Rust प्रोसेस के अंदर रखता है।

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

एक पाइपलाइन।  
कई एनालाइज़र।  
कम इंतज़ार।

---

## golangci-lint के साथ ड्रॉप-इन संगतता

पहले से `.golangci.yml` है?

बढ़िया।

उसे वैसे ही रखिए।

```bash
guff run ./...
```

guff अपने आप ये फ़ाइलें पढ़ लेता है:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

संगतता:

- ✅ golangci-lint v2 के 114 / 114 लिंटर लागू
- ✅ मौजूदा कॉन्फ़िगरेशन समर्थित
- ✅ कई आउटपुट फ़ॉर्मैट
- ✅ GitHub Actions एनोटेशन

ईमानदार तुलना (ज्ञात आंशिक अंतर सहित):

[`docs/COMPARE.md`](docs/COMPARE.md)

पूरा संगतता मैट्रिक्स:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

पाँच मिनट का माइग्रेशन + रोलबैक:

[`docs/MIGRATION.md`](docs/MIGRATION.md)

---

## AI कोडिंग एजेंट के लिए बना

AI कोडिंग एजेंट लगातार टूल चलाते रहते हैं।

धीमा लिंट कमांड पूरे डेवलपमेंट लूप को धीमा कर देता है।

guff इनके लिए डिज़ाइन किया गया है:

- Claude Code
- Cursor
- GitHub Copilot
- CI पाइपलाइन
- लोकल डेवलपमेंट

कॉपी-पेस्ट करने योग्य एजेंट निर्देश: [`docs/AGENTS.md`](docs/AGENTS.md) — इन्हें Claude Code के लिए `CLAUDE.md` में, Cursor के लिए `.cursor/rules` में, या अपने एजेंट के सिस्टम प्रॉम्प्ट में डाल दीजिए।

---

## अभी आज़माइए

### इंस्टॉल (Rust की ज़रूरत नहीं)

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh
```

डिफ़ॉल्ट रूप से `~/.local/bin` में इंस्टॉल होता है। उसके बाद:

```bash
guff run ./...
```

बस इतना ही। आपकी मौजूदा `.golangci.yml` काम करती रहेगी।

अन्य इंस्टॉलर:

```bash
# Homebrew
brew tap dakimura/guff https://github.com/dakimura/guff
brew install guff
```

Docker, aqua, Actions, cargo: [`docs/INSTALL.md`](docs/INSTALL.md).

### अनइंस्टॉल / रोलबैक

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/uninstall.sh | sh
```

कॉन्फ़िग फ़ाइलों को छुआ नहीं जाता — CI को कभी भी वापस golangci-lint पर मोड़ा जा सकता है। विवरण: [`docs/INSTALL.md`](docs/INSTALL.md#uninstall--rollback).

---

## आम कमांड

```bash
# Run configured linters
guff run ./...

# Show enabled linters
guff linters

# Use fast preset
guff run --preset fast ./...

# Enable additional linters
guff run \
  --enable revive \
  --enable misspell \
  ./...

# Apply suggested fixes
guff run --fix ./...

# Formatters (gofmt / goimports / … from config)
guff fmt .

# Re-lint on change (keeps analysis warm)
guff run --watch ./...

# Issues cache
guff cache status
guff cache clean
```

एडिटर, pre-commit, lefthook: [`docs/EDITORS.md`](docs/EDITORS.md).

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

यह Action, guff के विश्लेषण कैश को रन के बीच बनाए रखता है, इसलिए एक पुल रिक्वेस्ट सिर्फ़ अपने बदले हुए हिस्से को ही दोबारा लिंट करती है। GitHub-होस्टेड रनर पर 113 पैकेज वाले मॉड्यूल में कोल्ड रन 7.9s, बिना बदलाव वाला ट्री 0.2s, और व्यापक रूप से इम्पोर्ट होने वाली फ़ाइल में असली बदलाव 4.2s लेता है। मैट्रिक्स, कैश साइज़ का नॉब, और सेल्फ़-होस्टेड रनर: [`docs/CI.md`](docs/CI.md).

---

## Docker

पैकेज रिज़ॉल्यूशन के लिए guff को Go टूलचेन चाहिए।

आधिकारिक Docker इमेज में Go पहले से शामिल है।

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  ghcr.io/dakimura/guff:0.6.0 \
  run ./...
```

वैकल्पिक: रन के बीच Go कैश बनाए रखें:

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

# कॉन्फ़िगरेशन

guff मौजूदा `golangci-lint` कॉन्फ़िगरेशन फ़ाइलों को समर्थन देता है।

खोज का क्रम:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

उदाहरण:

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

चलाने के लिए:

```bash
guff run .
```

या कोई कॉन्फ़िग बताएँ:

```bash
guff run -c .golangci.yml .
```

v1 → v2: `guff migrate`.

---

# समर्थित लिंटर

guff, golangci-lint v2 का पूरा लिंटर सेट लागू करता है।

मौजूदा संगतता:

```
114 / 114 linters supported
```

उदाहरण:

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

अतिरिक्त लिंटर चालू करें:

```bash
guff run \
  --enable revive \
  --enable gosec \
  ./...
```

पूरा मैट्रिक्स:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

---

# आउटपुट फ़ॉर्मैट

समर्थित फ़ॉर्मैट:

- text
- colored-line-number
- json
- checkstyle
- sarif
- tab
- colored-tab
- github-actions

उदाहरण:

```bash
guff run \
  --out-format github-actions \
  ./...
```

GitHub Actions अपने आप पुल रिक्वेस्ट पर एनोटेशन जोड़ देगा।

---

# आर्किटेक्चर

guff एक साझा विश्लेषण पाइपलाइन के इर्द-गिर्द बना है।

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

पारंपरिक लिंट एग्रीगेटर के उलट, guff हर टूल के लिए विश्लेषण की स्थिति बार-बार नहीं बनाता।

नतीजा:

- कम स्टार्टअप ओवरहेड
- कम मेमोरी उपयोग
- तेज़ फ़ीडबैक लूप

---

# डेवलपमेंट

आवश्यकताएँ:

- Go
- Rust (edition 2021)

बिल्ड:

```bash
cargo build
```

टेस्ट:

```bash
cargo test
```

लोकल रन:

```bash
cargo run -p guff-lint -- run ./...
```

---

## बेंचमार्किंग

रिलीज़ बाइनरी बिल्ड करें:

```bash
cargo build --release -p guff-lint
```

बेंचमार्क चलाएँ:

```bash
./benchmarks/smoke.sh

./benchmarks/run.sh
```

OSS रिपॉज़िटरी बेंचमार्क:

```bash
./benchmarks/run.sh \
  --oss \
  --tier pr,nightly,weekly
```

परिणाम:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## संगतता परीक्षण

guff लगातार अपने परिणामों की तुलना golangci-lint से करता रहता है।

संगतता जाँच चलाएँ:

```bash
./compat/run.sh \
  --oss \
  --tier pr
```

प्रति-लिंटर आइसोलेट (एक बार में एक ही लिंटर चालू):

```bash
./compat/run.sh --isolate --smoke
./compat/run.sh --isolate
```

लक्ष्य:

> वही कॉन्फ़िग। वही निष्कर्ष। कहीं तेज़ निष्पादन।

---

## Prometheus रिग्रेशन गेट

guff में Prometheus के विरुद्ध एक रिग्रेशन सूट शामिल है।

यह जाँचता है:

- निष्पादन समय
- पीक RSS मेमोरी
- निष्कर्षों में अंतर

चलाएँ:

```bash
./regress/run.sh
```

पूरा प्रोफ़ाइल:

```bash
./regress/run.sh \
  --profile full
```

---

# सोर्स लेआउट

Cargo वर्कस्पेस की संरचना:

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

मुख्य घटक:

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

# लाइसेंस

GPL-3.0

CI में या लोकल मशीन पर `guff` CLI इस्तेमाल करने से आपका Go ऐप्लिकेशन GPL के अधीन **नहीं** हो जाता। विवरण: [`docs/LICENSE-FAQ.md`](docs/LICENSE-FAQ.md).

रिलीज़ सत्यापन / SBOM: [`docs/SUPPLY-CHAIN.md`](docs/SUPPLY-CHAIN.md).

guff में कई upstream Go प्रोजेक्ट्स से एनालाइज़र के पोर्ट और अनुकूलन शामिल हैं।

एट्रिब्यूशन और लाइसेंस जानकारी के लिए देखें:

- [`LICENSE`](LICENSE)
- [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)
