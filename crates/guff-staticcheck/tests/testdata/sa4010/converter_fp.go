package converterfp

// Mirrors grafana converter.readLabelsOrExemplars control flow around the
// pairs append that SA4010 falsely flags.

func readLabelsAsPairs() ([][2]string, error) { return nil, nil }

func readLabelsOrExemplars(keys []string) (int, [][2]string, error) {
	pairs := make([][2]string, 0, 10)
	frame := 0

l1Fields:
	for _, l1Field := range keys {
		switch l1Field {
		case "seriesLabels":
			_ = l1Field

		case "exemplars":
			lookup := make(map[string]int)
			exCount := 0
			for j := 0; j < 2; j++ {
				for _, l2Field := range []string{"labels", "other"} {
					switch l2Field {
					case "labels":
						pairs, err := readLabelsAsPairs()
						if err != nil {
							return 0, nil, err
						}
						for _, pair := range pairs {
							k := pair[0]
							if _, ok := lookup[k]; !ok {
								lookup[k] = exCount
							}
							_ = pair[1]
						}
					default:
						_ = l2Field
					}
				}
				exCount++
			}
		case "":
			break l1Fields

		default:
			v := l1Field
			pairs = append(pairs, [2]string{l1Field, v})
		}
	}

	return frame, pairs, nil
}
