package fixtures

// With upperCaseConst: true, SCREAMING_SNAKE consts are allowed.

const SOME_CONST_2 = 2
const _SOME_PRIVATE_CONST_2 = 2

const (
	SOME_CONST_3          = 3
	_SOME_PRIVATE_CONST_3 = 3
	VER                   = 0
)

// Still flagged: not a const (ALL_CAPS var).
var BAD_VAR_NAME = 1
