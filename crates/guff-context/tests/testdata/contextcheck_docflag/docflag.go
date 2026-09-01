package docflag

import "context"

func consume(ctx context.Context) { _ = ctx }

//nolint:contextcheck // upstream's own directive: skip this function entirely
func skipped() {
	consume(context.Background())
}

// CallsSkipped hears nothing, because a skipped function records no verdict at
// all — this is not golangci-lint's `//nolint` processor, which would only
// have covered the directive's own declaration.
func CallsSkipped(ctx context.Context) {
	consume(ctx)
	skipped()
}

// nolint:contextcheck // one space after the slashes still matches `^//\s?nolint:`
func skippedSpaced() {
	consume(context.Background())
}

func CallsSkippedSpaced(ctx context.Context) {
	consume(ctx)
	skippedSpaced()
}

//nolint:gosec // another linter, and the word is not in the text
func notSkippedOtherLinter() {
	consume(context.Background())
}

func CallsOtherLinter(ctx context.Context) {
	consume(ctx)
	notSkippedOtherLinter()
}

// nolint : contextcheck // a space before the colon does not match `^//\s?nolint:`
func notSkippedSpacedColon() {
	consume(context.Background())
}

func CallsSpacedColon(ctx context.Context) {
	consume(ctx)
	notSkippedSpacedColon()
}

// plainDoc has a doc comment with no directive in it at all.
func plainDoc() {
	consume(context.Background())
}

func CallsPlainDoc(ctx context.Context) {
	consume(ctx)
	plainDoc()
}
