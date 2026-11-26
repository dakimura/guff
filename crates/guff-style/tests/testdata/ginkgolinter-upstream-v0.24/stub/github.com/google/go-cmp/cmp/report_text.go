package cmp

import (
	"fmt"
	"math/rand"
	"time"
)

var randBool = rand.New(rand.NewSource(time.Now().Unix())).Intn(2) == 0

const maxColumnLength = 80

type indentMode int

func (n indentMode) appendIndent(b []byte, d diffMode) []byte {
	panic("stub")
}

type repeatCount int

func (n repeatCount) appendChar(b []byte, c byte) []byte {
	panic("stub")
}

type textNode interface {
	Len() int
	Equal(textNode) bool
	String() string
	formatCompactTo([]byte, diffMode) ([]byte, textNode)
	formatExpandedTo([]byte, diffMode, indentMode) []byte
}
type textWrap struct {
	Prefix   string
	Value    textNode
	Suffix   string
	Metadata interface{}
}

func (s *textWrap) Len() int {
	panic("stub")
}
func (s1 *textWrap) Equal(s2 textNode) bool {
	panic("stub")
}
func (s *textWrap) String() string {
	panic("stub")
}
func (s *textWrap) formatCompactTo(b []byte, d diffMode) ([]byte, textNode) {
	panic("stub")
}
func (s *textWrap) formatExpandedTo(b []byte, d diffMode, n indentMode) []byte {
	panic("stub")
}

type textList []textRecord
type textRecord struct {
	Diff       diffMode
	Key        string
	Value      textNode
	ElideComma bool
	Comment    fmt.Stringer
}

func (s *textList) AppendEllipsis(ds diffStats) {
	panic("stub")
}
func (s textList) Len() (n int) {
	panic("stub")
}
func (s1 textList) Equal(s2 textNode) bool {
	panic("stub")
}
func (s textList) String() string {
	panic("stub")
}
func (s textList) formatCompactTo(b []byte, d diffMode) ([]byte, textNode) {
	panic("stub")
}
func (s textList) formatExpandedTo(b []byte, d diffMode, n indentMode) []byte {
	panic("stub")
}
func (s textList) alignLens(skipFunc func(textRecord) bool, lenFunc func(textRecord) int) []repeatCount {
	panic("stub")
}

type textLine []byte

var (
	textNil      = textLine("nil")
	textEllipsis = textLine("...")
)

func (s textLine) Len() int {
	panic("stub")
}
func (s1 textLine) Equal(s2 textNode) bool {
	panic("stub")
}
func (s textLine) String() string {
	panic("stub")
}
func (s textLine) formatCompactTo(b []byte, d diffMode) ([]byte, textNode) {
	panic("stub")
}
func (s textLine) formatExpandedTo(b []byte, _ diffMode, _ indentMode) []byte {
	panic("stub")
}

type diffStats struct {
	Name         string
	NumIgnored   int
	NumIdentical int
	NumRemoved   int
	NumInserted  int
	NumModified  int
}

func (s diffStats) IsZero() bool {
	panic("stub")
}
func (s diffStats) NumDiff() int {
	panic("stub")
}
func (s diffStats) Append(ds diffStats) diffStats {
	panic("stub")
}
func (s diffStats) String() string {
	panic("stub")
}

type commentString string

func (s commentString) String() string {
	panic("stub")
}
