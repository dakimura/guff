# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| controller-runtime | 313 | 305 | 298 | 95.2% | 97.7% | 0 |
| vault | 163 | 163 | 163 | 100.0% | 100.0% | 0 ** |
| kubernetes | 5 | 5 | 5 | 100.0% | 100.0% | 0 ** |

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

## controller-runtime

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goconst | 294 | 286 | 279 | 94.9% | 97.6% |
| govet | 5 | 5 | 5 | 100.0% | 100.0% |
| modernize | 1 | 1 | 1 | 100.0% | 100.0% |
| nolintlint | 1 | 1 | 1 | 100.0% | 100.0% |
| staticcheck | 12 | 12 | 12 | 100.0% | 100.0% |

### Allowed known diffs (22)
- guff-only: `pkg/client/fake/client.go:1428:goconst:string `ConfigMap` has 9 occurrences, make it a constant`
- guff-only: `pkg/client/fake/client.go:1446:goconst:string `node` has 10 occurrences, make it a constant`
- guff-only: `pkg/client/fake/client_test.go:1595:goconst:string `node` has 10 occurrences, make it a constant`
- guff-only: `pkg/client/fake/client_test.go:618:goconst:string `ConfigMap` has 9 occurrences, make it a constant`
- guff-only: `pkg/envtest/crd.go:391:goconst:string `v1beta1` has 17 occurrences, make it a constant`
- guff-only: `pkg/envtest/envtest_suite_test.go:61:goconst:string `ValidatingWebhookConfiguration` has 3 occurrences, make it a constant`
- guff-only: `pkg/envtest/envtest_suite_test.go:84:goconst:string `default` has 4 occurrences, make it a constant`
- guff-only: `pkg/envtest/envtest_test.go:106:goconst:string `v1beta1` has 17 occurrences, make it a constant`
- … and 14 more (see `compat/allowlists/`)

## vault

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 23 | 23 | 23 | 100.0% | 100.0% |
| govet | 63 | 63 | 63 | 100.0% | 100.0% |
| ineffassign | 2 | 2 | 2 | 100.0% | 100.0% |
| staticcheck | 71 | 71 | 71 | 100.0% | 100.0% |
| unused | 4 | 4 | 4 | 100.0% | 100.0% |

### Stale allowlist entries (no longer a diff: delete them)
- guff-only: `helper/pgpkeys/flag_test.go:118:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:124:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:31:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:42:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:50:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:58:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:81:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:87:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:91:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pgpkeys/flag_test.go:95:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pkcs7/sign_test.go:115:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pkcs7/sign_test.go:131:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pkcs7/sign_test.go:163:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/pkcs7/sign_test.go:269:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/testhelpers/teststorage/teststorage.go:106:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/testhelpers/teststorage/teststorage.go:75:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.26) into a file using go1.24.3`
- guff-only: `helper/testhelpers/teststorage/teststorage_reusable.go:163:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.26) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:118:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:124:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:31:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:42:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:50:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:58:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:81:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:87:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:91:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pgpkeys/flag_test.go:95:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pkcs7/sign_test.go:115:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pkcs7/sign_test.go:131:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pkcs7/sign_test.go:163:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/pkcs7/sign_test.go:269:govet:inline: cannot inline call to ioutil.TempFile (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/testhelpers/teststorage/teststorage.go:106:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/testhelpers/teststorage/teststorage.go:75:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.27) into a file using go1.24.3`
- golangci-only: `helper/testhelpers/teststorage/teststorage_reusable.go:163:govet:inline: cannot inline call to ioutil.TempDir (declared using go1.27) into a file using go1.24.3`

## kubernetes

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 5 | 5 | 5 | 100.0% | 100.0% |

### Stale allowlist entries (no longer a diff: delete them)
- guff-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:337:govet:inline: cannot inline call to ioutil.ReadFile (declared using go1.26) into a file using go1.24.0`
- guff-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:339:govet:inline: cannot inline call to ioutil.ReadFile (declared using go1.26) into a file using go1.24.0`
- guff-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:341:govet:inline: cannot inline call to ioutil.ReadFile (declared using go1.26) into a file using go1.24.0`
- guff-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:363:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.26) into a file using go1.24.0`
- guff-only: `staging/src/k8s.io/apimachinery/pkg/runtime/serializer/streaming/streaming_test.go:44:govet:inline: cannot inline call to ioutil.NopCloser (declared using go1.26) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:337:govet:inline: cannot inline call to ioutil.ReadFile (declared using go1.27) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:339:govet:inline: cannot inline call to ioutil.ReadFile (declared using go1.27) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:341:govet:inline: cannot inline call to ioutil.ReadFile (declared using go1.27) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/api/apitesting/roundtrip/compatibility.go:363:govet:inline: cannot inline call to ioutil.WriteFile (declared using go1.27) into a file using go1.24.0`
- golangci-only: `staging/src/k8s.io/apimachinery/pkg/runtime/serializer/streaming/streaming_test.go:44:govet:inline: cannot inline call to ioutil.NopCloser (declared using go1.27) into a file using go1.24.0`
