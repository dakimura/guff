package time

type Time struct{}

type parseError struct{}

func (parseError) Error() string { return "" }

func Parse(layout, value string) (Time, error) {
	return Time{}, parseError{}
}
