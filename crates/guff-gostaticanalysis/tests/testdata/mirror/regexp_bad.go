package mirror

import "regexp"

func badRegexp(re *regexp.Regexp) {
	_ = re.Match([]byte("x"))
	_ = re.MatchString(string([]byte("y")))
}
