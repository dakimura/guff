//! `guff` CLI binary.

// The analysis phase is allocation-heavy and highly parallel (buildir builds SSA
// over a whole type arena; N workers churn large allocs at once). macOS's system
// allocator serializes badly under that load; once the scheduler bottleneck was
// removed it was the next ceiling. mimalloc scales across threads — ~19% faster
// on the Prometheus `./...` run (16.5s → 13.4s) with no findings change.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::process::ExitCode {
    guff_lint::cli::main()
}
