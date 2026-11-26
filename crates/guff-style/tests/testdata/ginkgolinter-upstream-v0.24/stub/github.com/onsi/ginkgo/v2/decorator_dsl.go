package ginkgo

import (
	"github.com/onsi/ginkgo/v2/internal"
	"github.com/onsi/ginkgo/v2/types"
)

type Offset = internal.Offset
type FlakeAttempts = internal.FlakeAttempts
type MustPassRepeatedly = internal.MustPassRepeatedly

const Focus = internal.Focus
const Pending = internal.Pending
const Serial = internal.Serial
const Ordered = internal.Ordered
const ContinueOnFailure = internal.ContinueOnFailure
const OncePerOrdered = internal.OncePerOrdered

func Label(labels ...string) Labels {
	panic("stub")
}

type Labels = internal.Labels

func SemVerConstraint(semVerConstraints ...string) SemVerConstraints {
	panic("stub")
}

type SemVerConstraints = internal.SemVerConstraints

func ComponentSemVerConstraint(component string, semVerConstraints ...string) ComponentSemVerConstraints {
	panic("stub")
}

type ComponentSemVerConstraints = internal.ComponentSemVerConstraints
type PollProgressAfter = internal.PollProgressAfter
type PollProgressInterval = internal.PollProgressInterval
type NodeTimeout = internal.NodeTimeout
type SpecTimeout = internal.SpecTimeout
type GracePeriod = internal.GracePeriod
type SpecPriority = internal.SpecPriority

const SuppressProgressReporting = internal.SuppressProgressReporting

func AroundNode[F types.AroundNodeAllowedFuncs](f F) types.AroundNodeDecorator {
	panic("stub")
}
