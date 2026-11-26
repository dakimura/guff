package formatter

import (
	"os"
	"regexp"
)

var ColorableStdOut = newColorable(os.Stdout)
var ColorableStdErr = newColorable(os.Stderr)

const COLS = 80

type ColorMode uint8

const (
	ColorModeNone ColorMode = iota
	ColorModeTerminal
	ColorModePassthrough
)

var SingletonFormatter = New(ColorModeTerminal)

func F(format string, args ...any) string {
	panic("stub")
}
func Fi(indentation uint, format string, args ...any) string {
	panic("stub")
}
func Fiw(indentation uint, maxWidth uint, format string, args ...any) string {
	panic("stub")
}

type Formatter struct {
	ColorMode                ColorMode
	colors                   map[string]string
	styleRe                  *regexp.Regexp
	preserveColorStylingTags bool
}

func NewWithNoColorBool(noColor bool) Formatter {
	panic("stub")
}
func New(colorMode ColorMode) Formatter {
	panic("stub")
}
func (f Formatter) F(format string, args ...any) string {
	panic("stub")
}
func (f Formatter) Fi(indentation uint, format string, args ...any) string {
	panic("stub")
}
func (f Formatter) Fiw(indentation uint, maxWidth uint, format string, args ...any) string {
	panic("stub")
}
func (f Formatter) length(styled string) uint {
	panic("stub")
}
func (f Formatter) CycleJoin(elements []string, joiner string, cycle []string) string {
	panic("stub")
}
func (f Formatter) style(s string) string {
	panic("stub")
}
