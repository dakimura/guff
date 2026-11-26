package p

const UniqueOne = "unique value one"
const UniqueTwo = "unique value two"

func okFindDuplicates() {
	const ScopedUnique = "scoped unique value"
	_ = UniqueOne
	_ = UniqueTwo
	_ = ScopedUnique
}
