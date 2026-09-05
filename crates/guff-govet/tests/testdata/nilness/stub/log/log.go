// Package log is the fixture stub for `log.Fatalf`, one of the names
// `ctrlflow`'s STDLIB_NO_RETURN table proves cannot return. The body is empty
// on purpose: the property comes from the table, not from the source, exactly
// as it does upstream — where it comes from a fact exported by the real `log`.
package log

func Fatalf(format string, v ...any) {}

func Println(v ...any) {}
