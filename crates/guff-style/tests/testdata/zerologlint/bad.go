package zerologlint

import "github.com/rs/zerolog/log"

func badBasic() {
	log.Info()
}

func badChain() {
	log.Info().Str("foo", "bar")
}

func badReassign() {
	logger := log.Info()
	logger = log.Error()
	logger.Str("foo", "bar")
}

func badDefer() {
	defer log.Info()
}
