package bad

// honnef unused (10.1): an exported const in the block keeps unexported
// siblings from being reported (k9s vulIdx / NodeUnreachablePodReason).
const (
	ExportedReason = "used"
	vulIdx         = 2
)

func Run() {
	_ = ExportedReason
}
