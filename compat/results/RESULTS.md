# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| vault | 125 | 161 | 125 | 100.0% | 77.6% | 0 |
| kubernetes | 0 | 5 | 0 | 100.0% | 0.0% | 0 |

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
| errcheck | 23 | 23 | 23 | 100.0% | 100.0% |
| govet | 46 | 63 | 46 | 100.0% | 73.0% |
| ineffassign | 2 | 2 | 2 | 100.0% | 100.0% |
| staticcheck | 50 | 69 | 50 | 100.0% | 72.5% |
| unused | 4 | 4 | 4 | 100.0% | 100.0% |

### Allowed known diffs (36)
- golangci-only: `helper/forwarding/util.go:15:staticcheck:"github.com/golang/protobuf/proto" is deprecated: Use the "google.golang.org/protobuf/proto" package instead.`
- golangci-only: `helper/identity/identity.go:9:staticcheck:"github.com/golang/protobuf/proto" is deprecated: Use the "google.golang.org/protobuf/proto" package instead.`
- golangci-only: `helper/identity/mfa/mfa.go:9:staticcheck:"github.com/golang/protobuf/proto" is deprecated: Use the "google.golang.org/protobuf/proto" package instead.`
- golangci-only: `helper/identity/sentinel.go:109:staticcheck:ptypes.TimestampString is deprecated: Call the ts.AsTime method instead, followed by a call to the Format method on the time.Time value.`
- golangci-only: `helper/identity/sentinel.go:111:staticcheck:ptypes.TimestampString is deprecated: Call the ts.AsTime method instead, followed by a call to the Format method on the time.Time value.`
- golangci-only: `helper/identity/sentinel.go:22:staticcheck:ptypes.TimestampString is deprecated: Call the ts.AsTime method instead, followed by a call to the Format method on the time.Time value.`
- golangci-only: `helper/identity/sentinel.go:24:staticcheck:ptypes.TimestampString is deprecated: Call the ts.AsTime method instead, followed by a call to the Format method on the time.Time value.`
- golangci-only: `helper/identity/sentinel.go:66:staticcheck:ptypes.TimestampString is deprecated: Call the ts.AsTime method instead, followed by a call to the Format method on the time.Time value.`
- … and 28 more (see `compat/allowlists/`)

## kubernetes

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 0 | 5 | 0 | 100.0% | 0.0% |

### Allowed known diffs (5)
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:337:govet:cannot inline call to ioutil.ReadFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:339:govet:cannot inline call to ioutil.ReadFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:341:govet:cannot inline call to ioutil.ReadFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:363:govet:cannot inline call to ioutil.WriteFile (declared using go1.26.2) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/runtime/serializer/streaming/streaming_test.go:44:govet:cannot inline call to ioutil.NopCloser (declared using go1.26.2) into a file using go1.24.0`
