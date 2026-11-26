package zerologlint_ok

import (
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

func okBasic() {
	log.Info().Msg("hi")
}

func okSend() {
	log.Info().Send()
}

func okReassign() {
	logger := log.Info()
	if false {
		logger = log.Error()
	}
	logger.Msg("hi")
}

func okLoggerRecv() {
	var l zerolog.Logger
	l.Info().Send()
}

func dispatcher(e *zerolog.Event) {
	e.Send()
}

func okDispatchInFunc() {
	event := log.Info()
	dispatcher(event)
}
