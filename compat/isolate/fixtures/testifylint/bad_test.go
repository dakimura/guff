package testifylint

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestBad(t *testing.T) {
	assert.Equal(t, false, true)
	assert.Nil(t, (*int)(nil))
	require.Error(t, nil)
	assert.Len(t, []int{1}, 0)
}
