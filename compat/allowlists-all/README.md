# `default: all` tier allowlist — deliberately empty

Used only by `./compat/run.sh --oss --tier pr --all-linters` (COMPAT-HARDENING
Phase 2). It is a **separate tree** from `compat/allowlists/` on purpose.

The all-linters tier is a discovery run: it turns on 114 linters over eight real
repositories, and its diffs *are* the work item, not noise to be silenced. If its
entries lived in the OSS allowlist they would also widen what the normal OSS gate
tolerates — a gate that is currently at P = R = 100% and should stay that way for
reasons that have nothing to do with this tier.

So the rule here is the same as the golden tier's: a diff is resolved by fixing
guff, not by adding a line to this directory. Anything that does land here needs
a comment explaining why it is a permanent, justified incompatibility rather than
an unfinished port.
