package types

type SemVerFilter func(component string, constraints []string) bool

func MustParseSemVerFilter(input string) SemVerFilter {
	panic("stub")
}
func ParseSemVerFilter(componentFilterVersions string) (SemVerFilter, error) {
	panic("stub")
}
func ValidateAndCleanupSemVerConstraint(semVerConstraint string, cl CodeLocation) (string, error) {
	panic("stub")
}
