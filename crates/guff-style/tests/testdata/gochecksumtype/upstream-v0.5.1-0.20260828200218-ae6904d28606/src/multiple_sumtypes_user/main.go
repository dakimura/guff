package multiple_sumtypes_user

import sumtypes "multiple_sumtypes"

func checkFirst(value sumtypes.First) {
	switch value.(type) { // want `exhaustiveness check failed for sum type "First" .*missing cases for FirstB`
	case *sumtypes.FirstA:
	}
}

func checkSecond(value sumtypes.Second) {
	switch value.(type) { // want `exhaustiveness check failed for sum type "Second" .*missing cases for SecondB`
	case *sumtypes.SecondA:
	}
}
