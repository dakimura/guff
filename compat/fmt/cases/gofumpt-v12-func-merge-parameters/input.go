package p

type f func(x int, y int) int

type i interface {
	add(x int, y int)
}

type s struct {
	x int
	y int
}

func mergeAdjacent(x int, y int) {}

func mergeThreeAdjacent(x int, y int, z int) {}

func mergeOneWithTwo(x, y int, z int) {}

func mergeTwoWithOne(x int, y, z int) {}

func mergeWithComment(
	x int, y int, // comment
)

func mergeAllSyntax(x chan []*foo.Bar, y chan []*foo.Bar) {}

func dontMergeAnonymousParams(int, int) {}

func dontMergeMultipleLines(
	x int,
	y int,
) {
}

func dontMergeMultipleLines2(
	x,
	y int,
	z int,
) {
}

func dontMergeDifferentKinds(format string, args ...string) {}

func dontMergeDifferentTypesReturn() (n int, err error) {}

func mergeIgnoresPositions(a func(x int), b func(
	x int)) {
}

type xint int

func dontMergeDistinguishedTypes(a func(x int), b func(xint)) {}
