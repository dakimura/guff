package time

type Time struct{}

func (t Time) Format(layout string) string {
	return ""
}

func Parse(layout, value string) (Time, error) {
	return Time{}, nil
}
