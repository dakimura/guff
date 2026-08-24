package golines

import "fmt"

// Over the 60-column budget in three different ways: a long signature, a long
// call chain, and a long struct tag line.
func LongSignature(alpha string, beta string, gamma string, delta string) string {
	return alpha + beta + gamma + delta
}

type Tagged struct {
	Field string `json:"field" yaml:"field" xml:"field" toml:"field"`
}

func Chained(values []string) string {
	return fmt.Sprintf("%s-%s-%s-%s", values[0], values[1], values[2], values[3])
}

// A second over-long line does **not** add a finding: golines is a formatter,
// and formatters report once per file ("File is not properly formatted"), not
// once per place they would change. The extra line is here so the fixture
// covers a long call chain as well as a long signature — same finding, more
// reformatting behind it.
func LongCall(alpha string, beta string) string {
	return fmt.Sprintf("%s-%s-%s-%s-%s-%s", alpha, beta, alpha, beta, alpha, beta)
}
