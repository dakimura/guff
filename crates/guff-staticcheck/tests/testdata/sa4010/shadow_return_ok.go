package shadowreturn

func other() ([][2]string, error) { return nil, nil }

func readLabelsOrExemplars(n int, key string) (*int, [][2]string, error) {
	pairs := make([][2]string, 0, 10)
	var frame *int

l1Fields:
	for i := 0; i < n; i++ {
		switch key {
		case "seriesLabels":
			_ = key
		case "exemplars":
			for j := 0; j < 2; j++ {
				switch j {
				case 0:
					pairs, err := other()
					if err != nil {
						return nil, nil, err
					}
					for _, pair := range pairs {
						_ = pair
					}
				default:
					_ = j
				}
			}
		case "":
			break l1Fields
		default:
			pairs = append(pairs, [2]string{key, "v"})
		}
	}
	return frame, pairs, nil
}
