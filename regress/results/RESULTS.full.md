# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 1.890 | 2.440 |
| peak_rss_bytes | 2,501,869,568 | 2,544,189,440 |
| guff_issues | 20 | 20 |
| golangci_issues | 20 | 0 |
| both | 20 | 0 |
| guff_only | 0 | 20 |
| golangci_only | 0 | 0 |
| precision | 1.0000 | 1.0000 |
| recall | 1.0000 | 1.0000 |

## FAIL

- `wall_seconds`: wall 2.440s > limit 2.040s (baseline 1.890s × 1.0 + 0.150s)
- `guff_only`: guff_only 20 > limit 0 (baseline 0)
- `both`: both 0 < floor 20 (baseline 20)
