package testifylint

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// enabled is a package constant whose value is true. It is not the predeclared
// `true`, and `isUntypedTrue` is object identity against
// `types.Universe.Lookup("true")` — so the assertion below is not the
// `assert.Equal(t, true, x)` that bool-compare rewrites.
const enabled = true

func TestBad(t *testing.T) {
	assert.Equal(t, false, true)
	assert.Nil(t, (*int)(nil))
	require.Error(t, nil)
	assert.Len(t, []int{1}, 0)
}

func constBoolCompare(t *testing.T, got bool) {
	assert.Equal(t, enabled, got)
}
