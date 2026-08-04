package p

func Bad() {
	i := 0
	i += 1
}

func exportedWithoutDoc() {}

// DocumentedFlags is a block comment that must silence exported on the consts
// (load uses Mode::NONE; revive must PARSE_COMMENTS-reparse to see it).
const (
	DocumentedFlagA = 1
	DocumentedFlagB = 2
)

// DocumentedAlone is a single const with a proper doc.
const DocumentedAlone = 3

// DocumentedFunc is documented.
func DocumentedFunc() {}

const UndocumentedAlone = 4

func BadNames() {
	var Id int
	_ = Id
}
