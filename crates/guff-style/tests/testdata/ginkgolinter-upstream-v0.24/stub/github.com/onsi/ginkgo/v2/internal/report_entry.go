package internal

import (
	"github.com/onsi/ginkgo/v2/types"
)

type ReportEntry = types.ReportEntry

func NewReportEntry(name string, cl types.CodeLocation, args ...any) (ReportEntry, error) {
	panic("stub")
}
