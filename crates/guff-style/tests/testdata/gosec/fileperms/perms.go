// Package fileperms exercises the three gosec rules whose threshold comes from
// `linters.settings.gosec.config` (`rules/fileperms.go`): each call is either a
// subset of the configured mode (silent) or not (reported), and the message
// text carries the configured mode rather than the rule default.
package fileperms

import "os"

// Subsets of the configured modes — silent under the case's config, reported
// under gosec's defaults (0750 / 0600 / 0600).
func MkdirSubset() error             { return os.Mkdir("d", 0755) }
func MkdirAllSubset() error          { return os.MkdirAll("d", 0777) }
func ChmodSubset() error             { return os.Chmod("f", 0644) }
func WriteFileSubset(b []byte) error { return os.WriteFile("f", b, 0644) }

// Not subsets of the configured modes — reported either way, and the message
// is what pins the threshold that was in force.
func MkdirSticky() error                 { return os.Mkdir("d", 01777) }
func ChmodWorldWrite() error             { return os.Chmod("f", 0666) }
func WriteFileWorldWrite(b []byte) error { return os.WriteFile("f", b, 0666) }
