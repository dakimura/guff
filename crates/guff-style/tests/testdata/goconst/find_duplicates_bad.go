package p

const DuplicateConst1 = "duplicate value"
const DuplicateConst2 = "duplicate value"

const (
	GroupedDuplicateConst1 = "grouped duplicate value"
	GroupedDuplicateConst2 = "grouped duplicate value"
)

func badFindDuplicates() {
	const ScopedDuplicateConst1 = "duplicate scoped value"
	const ScopedDuplicateConst2 = "duplicate scoped value"
	_ = DuplicateConst1
	_ = DuplicateConst2
	_ = GroupedDuplicateConst1
	_ = GroupedDuplicateConst2
	_ = ScopedDuplicateConst1
	_ = ScopedDuplicateConst2
}
