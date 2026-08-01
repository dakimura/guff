# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| consul | 246 | 255 | 246 | 100.0% | 96.5% | 0 |
| grafana | 21 | 0 | 0 | 0.0% | 100.0% | 0 |
| containerd | 1 | 1 | 1 | 100.0% | 100.0% | 0 |

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

## consul

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 9 | 18 | 9 | 100.0% | 50.0% |
| staticcheck | 237 | 237 | 237 | 100.0% | 100.0% |

### Allowed known diffs (9)
- golangci-only: `agent/grpc-external/services/resource/write.go:206:govet:cannot inline: type parameter inference is not yet supported`
- golangci-only: `agent/grpc-external/services/resource/write.go:207:govet:cannot inline: type parameter inference is not yet supported`
- golangci-only: `agent/grpc-external/services/resource/write.go:212:govet:cannot inline: type parameter inference is not yet supported`
- golangci-only: `agent/structs/acl_templated_policy.go:303:govet:cannot inline: type parameter inference is not yet supported`
- golangci-only: `agent/structs/config_entry_gateways.go:1008:govet:cannot inline: type parameter inference is not yet supported`
- golangci-only: `agent/xds/routes.go:516:govet:cannot inline: type parameter inference is not yet supported`
- golangci-only: `command/snapshot/save/snapshot_save.go:80:govet:cannot inline: type parameter inference is not yet supported`
- golangci-only: `command/snapshot/save/snapshot_save.go:96:govet:cannot inline: type parameter inference is not yet supported`
- … and 1 more (see `compat/allowlists/`)

## grafana

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goimports | 1 | 0 | 0 | 0.0% | 100.0% |
| gosec | 3 | 0 | 0 | 0.0% | 100.0% |
| prealloc | 1 | 0 | 0 | 0.0% | 100.0% |
| staticcheck | 16 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (21)
- guff-only: `pkg/components/imguploader/gcs/gcsuploader.go:230:staticcheck:ineffective assignment to field .PredefinedACL`
- guff-only: `pkg/components/simplejson/simplejson_go11.go:4:staticcheck:identical build constraints "go1.1" and "go1.1"`
- guff-only: `pkg/infra/process/root_check.go:4:staticcheck:identical build constraints "!windows" and "!windows"`
- guff-only: `pkg/registry/apis/datasource/converter/converter_test.go:58:staticcheck:trying to marshal unsupported type github.com/grafana/grafana/pkg/services/datasources.UpdateSecretFn, via x.UpdateSecretFn`
- guff-only: `pkg/registry/apis/datasource/converter/converter_test.go:70:staticcheck:trying to marshal unsupported type github.com/grafana/grafana/pkg/services/datasources.UpdateSecretFn, via x.UpdateSecretFn`
- guff-only: `pkg/registry/apis/provisioning/jobs/migrate/unifiedstorage_test.go:458:staticcheck:identical expressions on the left and right side of the '&&' operator`
- guff-only: `pkg/registry/apis/secret/service/consolidation.go:165:prealloc:Consider preallocating currentBatch`
- guff-only: `pkg/registry/fieldselectors/selectable_fields_utils.go:51:staticcheck:robj refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted`
- … and 13 more (see `compat/allowlists/`)

## containerd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 1 | 1 | 1 | 100.0% | 100.0% |
