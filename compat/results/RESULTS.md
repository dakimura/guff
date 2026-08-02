# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| vault | 125 | 161 | 122 | 97.6% | 75.8% | 0 |
| kubernetes | 3 | 5 | 0 | 0.0% | 0.0% | 0 |

Precision = |intersection| / |guff|; Recall = |intersection| / |golangci|. `unexpected` counts diffs not covered by the allowlist (`compat/allowlists/`).

## fixture

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 2 | 2 | 2 | 100.0% | 100.0% |
| ineffassign | 1 | 1 | 1 | 100.0% | 100.0% |
| unused | 1 | 1 | 1 | 100.0% | 100.0% |

## local

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 12 | 12 | 12 | 100.0% | 100.0% |
| ineffassign | 12 | 12 | 12 | 100.0% | 100.0% |
| staticcheck | 72 | 72 | 72 | 100.0% | 100.0% |
| unused | 12 | 12 | 12 | 100.0% | 100.0% |

## vault

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 22 | 23 | 22 | 100.0% | 95.7% |
| govet | 46 | 63 | 46 | 100.0% | 73.0% |
| ineffassign | 2 | 2 | 2 | 100.0% | 100.0% |
| staticcheck | 51 | 69 | 48 | 94.1% | 69.6% |
| unused | 4 | 4 | 4 | 100.0% | 100.0% |

### Allowed known diffs (42)
- guff-only: `helper/testhelpers/testhelpers.go:599:staticcheck:possible nil pointer dereference`
- guff-only: `helper/testhelpers/testhelpers.go:827:staticcheck:possible nil pointer dereference`
- guff-only: `helper/testhelpers/testhelpers.go:919:staticcheck:possible nil pointer dereference`
- golangci-only: `helper/forwarding/util.go:15:staticcheck:"github.com/golang/protobuf/proto" is deprecated: Use the "google.golang.org/protobuf/proto" package instead.`
- golangci-only: `helper/identity/identity.go:9:staticcheck:"github.com/golang/protobuf/proto" is deprecated: Use the "google.golang.org/protobuf/proto" package instead.`
- golangci-only: `helper/identity/mfa/mfa.go:9:staticcheck:"github.com/golang/protobuf/proto" is deprecated: Use the "google.golang.org/protobuf/proto" package instead.`
- golangci-only: `helper/identity/sentinel.go:109:staticcheck:ptypes.TimestampString is deprecated: Call the ts.AsTime method instead, followed by a call to the Format method on the time.Time value.`
- golangci-only: `helper/identity/sentinel.go:111:staticcheck:ptypes.TimestampString is deprecated: Call the ts.AsTime method instead, followed by a call to the Format method on the time.Time value.`
- … and 34 more (see `compat/allowlists/`)

## kubernetes

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 0 | 5 | 0 | 100.0% | 0.0% |
| staticcheck | 3 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (8)
- guff-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/fuzzer/valuefuzz.go:36:staticcheck:empty branch`
- guff-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/fuzzer/valuefuzz.go:44:staticcheck:empty branch`
- guff-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/fuzzer/valuefuzz.go:54:staticcheck:empty branch`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:337:govet:cannot inline call to ioutil.ReadFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:339:govet:cannot inline call to ioutil.ReadFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:341:govet:cannot inline call to ioutil.ReadFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:363:govet:cannot inline call to ioutil.WriteFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/runtime/serializer/streaming/streaming_test.go:44:govet:cannot inline call to ioutil.NopCloser (declared using go1.26.2) into a file using go1.24.0`
