package types

import (
	"time"
)

type ReportEntryValue struct {
	raw            any
	AsJSON         string
	Representation string
}

func WrapEntryValue(value any) ReportEntryValue {
	panic("stub")
}
func (rev ReportEntryValue) GetRawValue() any {
	panic("stub")
}
func (rev ReportEntryValue) String() string {
	panic("stub")
}
func (rev ReportEntryValue) MarshalJSON() ([]byte, error) {
	panic("stub")
}
func (rev *ReportEntryValue) UnmarshalJSON(data []byte) error {
	panic("stub")
}
func (rev ReportEntryValue) GobEncode() ([]byte, error) {
	panic("stub")
}
func (rev *ReportEntryValue) GobDecode(data []byte) error {
	panic("stub")
}

type ReportEntry struct {
	Visibility       ReportEntryVisibility
	Location         CodeLocation
	Time             time.Time
	TimelineLocation TimelineLocation
	Name             string
	Value            ReportEntryValue
}
type ColorableStringer interface{ ColorableString() string }

func (entry ReportEntry) StringRepresentation() string {
	panic("stub")
}
func (entry ReportEntry) GetRawValue() any {
	panic("stub")
}
func (entry ReportEntry) GetTimelineLocation() TimelineLocation {
	panic("stub")
}

type ReportEntries []ReportEntry

func (re ReportEntries) HasVisibility(visibilities ...ReportEntryVisibility) bool {
	panic("stub")
}
func (re ReportEntries) WithVisibility(visibilities ...ReportEntryVisibility) ReportEntries {
	panic("stub")
}

type ReportEntryVisibility uint

const (
	ReportEntryVisibilityAlways ReportEntryVisibility = iota
	ReportEntryVisibilityFailureOrVerbose
	ReportEntryVisibilityNever
)

var revEnumSupport = NewEnumSupport(map[uint]string{uint(ReportEntryVisibilityAlways): "always", uint(ReportEntryVisibilityFailureOrVerbose): "failure-or-verbose", uint(ReportEntryVisibilityNever): "never"})

func (rev ReportEntryVisibility) String() string {
	panic("stub")
}
func (rev *ReportEntryVisibility) UnmarshalJSON(b []byte) error {
	panic("stub")
}
func (rev ReportEntryVisibility) MarshalJSON() ([]byte, error) {
	panic("stub")
}
func (v ReportEntryVisibility) Is(visibilities ...ReportEntryVisibility) bool {
	panic("stub")
}
