# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 58.430 | 58.430 |
| peak_rss_bytes | 11,217,780,736 | 11,217,780,736 |
| guff_issues | 476 | 476 |
| golangci_issues | 20 | 20 |
| both | 16 | 16 |
| guff_only | 460 | 460 |
| golangci_only | 4 | 4 |
| precision | 0.0336 | 0.0336 |
| recall | 0.8000 | 0.8000 |

## PASS

No regressions vs baseline (within tolerances).
