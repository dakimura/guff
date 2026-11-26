package p

import (
	"io"
	"io/ioutil" // if the user keeps them in the top group, obey that
	_ "io/ioutil"

	_ "image/png"

	"bufio" // the above is for a side effect; this one has a comment
)

import (
	"io"
	"os"

	"foo.local/one"

	bytes_ "bytes"
)

import (
	"fmt"

	"foo.local/two"
)

// If they are in order, but with extra newlines, join them.
import (
	"more"
	"std"
)

// We need to split std vs non-std in this case too.
import (
	math "math"

	"foo.local"
	"foo.local/three"
)

import (
	"x"
	// don't mess up this comment
	"y"
	// or many
	// of them
	"z"
)

// This used to crash gofumpt, as there's no space to insert an extra newline.
import (
	"std"

	"non.std/pkg"
)

// All of the extra imports below are known to not belong in std.
// For example/ and test/, see https://golang.org/issue/37641.
import (
	"internal/bar"
	"io"

	"example/foo"
	"test/baz"
)

import (
	"io"
	"nodomainmod"
	"nodomainmod/mod1/pkg1"
	"nodomainmod/mod2"
	"nodomainmodextra"
)

import (
	"io"
	"nodomainother/mod.withdot/pkg1"
)

// Moving std imports up must not leave empty lines behind; see issue 225.
import (
	"path/filepath"
	"time"

	"github.com/tinkerbell/tink/pkg/apis/core/v1alpha1"
	"k8s.io/apimachinery/pkg/types"
	"sigs.k8s.io/yaml"
)

// Never move an import with comments, as they would be left behind.
import (
	"strings"

	"foo.local/pkg"
	"os" // why we need os
)

import (
	"foo.local/pkg"

	// why we need path
	"path"
)
