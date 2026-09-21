module example.com/gomodguard_v2

go 1.22

require (
	example.com/bare v1.0.0
	example.com/onerec v1.0.0
	example.com/threerecs v1.0.0
	example.com/tworecs v1.0.0
	example.com/verbs v1.0.0
	github.com/sirupsen/logrus v1.9.3
)

replace github.com/sirupsen/logrus => ./logrus

replace example.com/bare => ./bare

replace example.com/onerec => ./onerec

replace example.com/tworecs => ./tworecs

replace example.com/threerecs => ./threerecs

replace example.com/verbs => ./verbs
