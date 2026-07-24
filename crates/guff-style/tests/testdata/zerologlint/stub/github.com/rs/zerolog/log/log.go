package log

import "github.com/rs/zerolog"

func Info() *zerolog.Event {
	return &zerolog.Event{}
}

func Error() *zerolog.Event {
	return &zerolog.Event{}
}
