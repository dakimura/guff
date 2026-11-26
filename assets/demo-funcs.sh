#!/usr/bin/env bash
# Shell functions sourced by the VHS demo (assets/demo.tape).
# Timings from benchmarks/results/SCOREBOARD.md — helm cold-cache.

golangci-lint() {
  local frames=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
  local times=('0.5s' '1.2s' '2.8s' '5.1s' '8.4s' '12s' '16s' '19s' '22.1s')
  local i frame
  for i in "${!times[@]}"; do
    frame="${frames[$((i % ${#frames[@]}))]}"
    printf '\r  \033[33m%s\033[0m  linting…  \033[2m%s\033[0m   ' "$frame" "${times[$i]}"
    sleep 0.32
  done
  printf '\r  \033[2mdone in 22.1s\033[0m          \n'
}

guff() {
  sleep 0.28
  printf '  \033[32m✓\033[0m done in \033[1m1.7s\033[0m\n'
}
