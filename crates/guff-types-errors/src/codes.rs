//! Port of `internal/types/errors/codes.go`.
//!
//! Numeric discriminants are stable and match the Go source; gaps in the
//! sequence (e.g. 29, 79, 80, 107, 147) correspond to retired/never-used codes
//! and are preserved as gaps so cross-language tooling can compare integer
//! values directly.

/// Error codes produced during type-checking.
///
/// Equivalent to `errors.Code`. The numeric values are part of the API and
/// must not change — add new codes at the end. New variants should be added
/// to [`Code::try_from`] as well.
#[repr(i16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    /// Occurs if an invalid syntax tree is provided to the type checker. It
    /// should never happen.
    InvalidSyntaxTree = -1,

    /// Reserved for errors that only apply while in self-test mode.
    Test = 1,
    /// A package name is the blank identifier `_`.
    BlankPkgName = 2,
    /// A file's package name doesn't match the package name already
    /// established by other files.
    MismatchedPkgName = 3,
    /// A package identifier is used outside of a selector expression.
    InvalidPkgUse = 4,
    /// An import path is not valid.
    BadImportPath = 5,
    /// Importing a package fails.
    BrokenImport = 6,
    /// The special import `"C"` is renamed.
    ImportCRenamed = 7,
    /// An import is unused.
    UnusedImport = 8,
    /// An invalid cycle is detected within the initialization graph.
    InvalidInitCycle = 9,
    /// An identifier is declared multiple times.
    DuplicateDecl = 10,
    /// A declaration cycle is not valid.
    InvalidDeclCycle = 11,
    /// A cycle in type definitions results in a type that is not
    /// well-defined. (Never emitted; retained for numeric stability.)
    InvalidTypeCycle = 12,
    /// A const declaration has a non-constant initializer.
    InvalidConstInit = 13,
    /// A const value cannot be converted to its target type.
    InvalidConstVal = 14,
    /// The underlying type in a const declaration is not a valid constant type.
    InvalidConstType = 15,
    /// The predeclared (untyped) value `nil` is used to initialize a variable
    /// declared without an explicit type.
    UntypedNilUse = 16,
    /// The number of values on the right-hand side of an assignment does not
    /// match the number of variables on the left-hand side.
    WrongAssignCount = 17,
    /// The left-hand side of an assignment is not assignable.
    UnassignableOperand = 18,
    /// A short variable declaration (`:=`) does not declare any new variables.
    NoNewVar = 19,
    /// An assignment operation (`+=`, `*=`, etc) does not have single-valued
    /// left-hand or right-hand side.
    MultiValAssignOp = 20,
    /// A value of type `T` is used as an interface, but `T` does not implement
    /// a required method.
    InvalidIfaceAssign = 21,
    /// A channel assignment is invalid.
    InvalidChanAssign = 22,
    /// The type of the right-hand side cannot be assigned to the variable
    /// being assigned.
    IncompatibleAssign = 23,
    /// Trying to assign to a struct field in a map value.
    UnaddressableFieldAssign = 24,
    /// The identifier used as the underlying type in a type declaration does
    /// not denote a type.
    NotAType = 25,
    /// An array length is not a constant value.
    InvalidArrayLen = 26,
    /// A method name is `_`.
    BlankIfaceMethod = 27,
    /// A map key type does not support `==` and `!=`.
    IncomparableMapKey = 28,

    // 29 = retired `InvalidIfaceEmbed` (no Rust variant).

    /// An embedded field is `*T` where `T` is itself a pointer-ish type.
    InvalidPtrEmbed = 30,
    /// A method declaration does not have exactly one receiver parameter.
    BadRecv = 31,
    /// A receiver type expression is not of the form `T` or `*T`, or `T` is a
    /// pointer type.
    InvalidRecv = 32,
    /// An identifier appears as both a field and method name.
    DuplicateFieldAndMethod = 33,
    /// Two methods on the same receiver type have the same name.
    DuplicateMethod = 34,
    /// A blank identifier is used as a value or type.
    InvalidBlank = 35,
    /// `iota` is used outside of a constant declaration.
    InvalidIota = 36,
    /// An `init` function is missing its body.
    MissingInitBody = 37,
    /// An `init` function declares parameters or results (deprecated;
    /// `InvalidInitDecl` is used instead).
    InvalidInitSig = 38,
    /// `init` is declared as anything other than a function.
    InvalidInitDecl = 39,
    /// `main` is declared as anything other than a function, in a main package.
    InvalidMainDecl = 40,
    /// A function returns too many values for the expression context.
    TooManyValues = 41,
    /// A type expression is used where a value expression is expected.
    NotAnExpr = 42,
    /// A float constant is truncated to an integer value.
    TruncatedFloat = 43,
    /// A numeric constant overflows its target type.
    NumericOverflow = 44,
    /// An operator is not defined for the operand types.
    UndefinedOp = 45,
    /// Operand types are incompatible in a binary operation.
    MismatchedTypes = 46,
    /// A division operation is provably a division by zero at compile time.
    DivByZero = 47,
    /// `++`/`--` applied to a non-numeric value.
    NonNumericIncDec = 48,
    /// The `&` operator is applied to an unaddressable expression.
    UnaddressableOperand = 49,
    /// A non-pointer value is indirected via `*`.
    InvalidIndirection = 50,
    /// An index operation is applied to a value that cannot be indexed.
    NonIndexableOperand = 51,
    /// An index argument is not an integer, is negative, or is out of bounds.
    InvalidIndex = 52,
    /// Constant indices in a slice expression are decreasing.
    SwappedSliceIndices = 53,
    /// A slice operation is applied to a value whose type is not sliceable.
    NonSliceableOperand = 54,
    /// A three-index slice expression (`a[x:y:z]`) is applied to a string.
    InvalidSliceExpr = 55,
    /// The right-hand side of a shift is non-integer, negative, or too large.
    InvalidShiftCount = 56,
    /// The shifted operand is not an integer.
    InvalidShiftOperand = 57,
    /// Receive from a value that is not a channel, or is send-only.
    InvalidReceive = 58,
    /// Send to a value that is not a channel, or is receive-only.
    InvalidSend = 59,
    /// An index is duplicated in a slice, array, or map literal.
    DuplicateLitKey = 60,
    /// A map literal is missing a key expression.
    MissingLitKey = 61,
    /// The key in a key-value element of a slice or array literal is not an
    /// integer constant.
    InvalidLitIndex = 62,
    /// An array literal exceeds its length.
    OversizeArrayLit = 63,
    /// A struct literal mixes positional and named elements.
    MixedStructLit = 64,
    /// A positional struct literal has an incorrect number of values.
    InvalidStructLit = 65,
    /// A struct literal refers to a non-existent field.
    MissingLitField = 66,
    /// A struct literal contains duplicated fields.
    DuplicateLitField = 67,
    /// A positional struct literal implicitly assigns an unexported field of
    /// an imported type.
    UnexportedLitField = 68,
    /// A field name is not a valid identifier.
    InvalidLitField = 69,
    /// A composite literal omits a required type identifier.
    UntypedLit = 70,
    /// A composite literal expression does not match its type.
    InvalidLit = 71,
    /// A selector is ambiguous.
    AmbiguousSelector = 72,
    /// A package-qualified identifier is undeclared by the imported package.
    UndeclaredImportedName = 73,
    /// A selector refers to an unexported identifier of an imported package.
    UnexportedName = 74,
    /// An identifier is not declared in the current scope.
    UndeclaredName = 75,
    /// A selector references a field or method that does not exist.
    MissingFieldOrMethod = 76,
    /// `...` occurs in a context where it is not valid.
    BadDotDotDotSyntax = 77,
    /// `...` is used on the final argument to a non-variadic function.
    NonVariadicDotDotDot = 78,

    // 79 = retired `MisplacedDotDotDot` (error reported by parser).
    // 80 = retired `InvalidDotDotDotOperand`.

    /// `...` is used in a non-variadic built-in function.
    InvalidDotDotDot = 81,
    /// A built-in function is used as a function-valued expression.
    UncalledBuiltin = 82,
    /// `append` is called with a first argument that is not a slice.
    InvalidAppend = 83,
    /// `cap` is called with an unsupported type.
    InvalidCap = 84,
    /// `close` is called with a non-channel, or a receive-only channel.
    InvalidClose = 85,
    /// `copy` arguments are not of slice type or have incompatible types.
    InvalidCopy = 86,
    /// `complex` is called with incompatible-typed arguments.
    InvalidComplex = 87,
    /// `delete` is called with a first argument that is not a map.
    InvalidDelete = 88,
    /// `imag` is called with a non-complex argument.
    InvalidImag = 89,
    /// `len` is called with an unsupported type.
    InvalidLen = 90,
    /// `make` is called with three args where length > capacity.
    SwappedMakeArgs = 91,
    /// `make` is called with an unsupported type argument.
    InvalidMake = 92,
    /// `real` is called with a non-complex argument.
    InvalidReal = 93,
    /// A type assertion is applied to a non-interface value.
    InvalidAssert = 94,
    /// A type assertion `x.(T)` is statically impossible.
    ImpossibleAssert = 95,
    /// An explicit conversion is not allowed by the spec.
    InvalidConversion = 96,
    /// No valid implicit conversion exists for an untyped value in context.
    InvalidUntypedConversion = 97,
    /// `unsafe.Offsetof` is called with a non-selector argument.
    BadOffsetofSyntax = 98,
    /// `unsafe.Offsetof` is called on a method selector or embedded via a
    /// pointer.
    InvalidOffsetof = 99,
    /// A side-effect-free expression is used as a statement.
    UnusedExpr = 100,
    /// A variable is declared but unused.
    UnusedVar = 101,
    /// A function with results is missing a `return` statement.
    MissingReturn = 102,
    /// A `return` statement returns an incorrect number of values.
    WrongResultCount = 103,
    /// The name of a value implicitly returned by an empty `return` is shadowed.
    OutOfScopeResult = 104,
    /// An `if` condition is not a boolean expression.
    InvalidCond = 105,
    /// There is a declaration in a for-loop post statement.
    InvalidPostDecl = 106,

    // 107 = retired `InvalidChanRange`.

    /// Two iteration variables are used while ranging over a channel.
    InvalidIterVar = 108,
    /// The type of a range expression is not valid for use with `range`.
    InvalidRangeExpr = 109,
    /// `break` is not within a `for`/`switch`/`select`.
    MisplacedBreak = 110,
    /// `continue` is not within a `for` loop.
    MisplacedContinue = 111,
    /// `fallthrough` is not within an expression switch.
    MisplacedFallthrough = 112,
    /// A type or expression switch has duplicate cases.
    DuplicateCase = 113,
    /// A type or expression switch has multiple default clauses.
    DuplicateDefault = 114,
    /// `.(type)` is used anywhere other than a type switch.
    BadTypeKeyword = 115,
    /// `.(type)` is used on an expression that is not of interface type.
    InvalidTypeSwitch = 116,
    /// A switch expression is not comparable.
    InvalidExprSwitch = 117,
    /// A `select` case is not a channel send or receive.
    InvalidSelectCase = 118,
    /// An undeclared label is jumped to.
    UndeclaredLabel = 119,
    /// A label is declared more than once.
    DuplicateLabel = 120,
    /// A break/continue label is not on a for/switch/select.
    MisplacedLabel = 121,
    /// A label is declared and not used.
    UnusedLabel = 122,
    /// A label jumps over a variable declaration.
    JumpOverDecl = 123,
    /// A forward jump goes to a label inside a nested block.
    JumpIntoBlock = 124,
    /// A pointer method is called but the argument is not addressable.
    InvalidMethodExpr = 125,
    /// Too few or too many arguments are passed by a function call.
    WrongArgCount = 126,
    /// An expression is called that is not of function type.
    InvalidCall = 127,
    /// A side-effect-free built-in is suspended via `go` or `defer`.
    UnusedResults = 128,
    /// A `defer` expression is not a function call.
    InvalidDefer = 129,
    /// A `go` expression is not a function call.
    InvalidGo = 130,

    /// A declaration has invalid syntax. (Added in Go 1.17.)
    BadDecl = 131,
    /// An identifier occurs more than once on the LHS of a short var decl.
    RepeatedDecl = 132,
    /// `unsafe.Add` is called with a non-integer length, or used pre-go1.17.
    InvalidUnsafeAdd = 133,
    /// `unsafe.Slice` has a bad pointer or length argument, or used pre-go1.17.
    InvalidUnsafeSlice = 134,

    /// A language feature is used that is not supported at this Go version.
    /// (Added in Go 1.18.)
    UnsupportedFeature = 135,
    /// A non-generic type is used where a generic type is expected.
    NotAGenericType = 136,
    /// A type/function is instantiated with the wrong number of type arguments.
    WrongTypeArgCount = 137,
    /// Type or function type argument inference fails.
    CannotInferTypeArgs = 138,
    /// A type argument does not satisfy its type parameter constraints.
    InvalidTypeArg = 139,
    /// An invalid cycle is detected within the instantiation graph.
    InvalidInstanceCycle = 140,
    /// An embedded union or approximation element is not valid.
    InvalidUnion = 141,
    /// A constraint-type interface is used outside of constraint position.
    MisplacedConstraintIface = 142,
    /// Methods have type parameters.
    InvalidMethodTypeParams = 143,
    /// A type parameter is used in a place where it is not permitted.
    MisplacedTypeParam = 144,
    /// `unsafe.SliceData` has a non-slice argument, or used pre-go1.20.
    InvalidUnsafeSliceData = 145,
    /// `unsafe.String` has a bad length, or used pre-go1.20.
    InvalidUnsafeString = 146,

    // 147 = retired `InvalidUnsafeStringData`.

    /// `clear` is called with an argument that is not of map or slice type.
    InvalidClear = 148,
    /// `unsafe.Sizeof`/`Offsetof` is called with a type that is too large.
    TypeTooLarge = 149,
    /// `min`/`max` is called with an operand that cannot be ordered.
    InvalidMinMaxOperand = 150,
    /// A source file requires a Go version newer than the type-checker logic.
    TooNew = 151,
}

impl Code {
    /// Returns the numeric value of this code (matches Go's `Code` integer
    /// values, suitable for tooling that compares codes across languages).
    pub fn to_i16(self) -> i16 {
        self as i16
    }

    /// Convert a numeric value back to a [`Code`], returning `None` for
    /// unknown or retired codes.
    pub fn from_i16(value: i16) -> Option<Self> {
        use Code::*;
        let v = match value {
            -1 => InvalidSyntaxTree,
            1 => Test,
            2 => BlankPkgName,
            3 => MismatchedPkgName,
            4 => InvalidPkgUse,
            5 => BadImportPath,
            6 => BrokenImport,
            7 => ImportCRenamed,
            8 => UnusedImport,
            9 => InvalidInitCycle,
            10 => DuplicateDecl,
            11 => InvalidDeclCycle,
            12 => InvalidTypeCycle,
            13 => InvalidConstInit,
            14 => InvalidConstVal,
            15 => InvalidConstType,
            16 => UntypedNilUse,
            17 => WrongAssignCount,
            18 => UnassignableOperand,
            19 => NoNewVar,
            20 => MultiValAssignOp,
            21 => InvalidIfaceAssign,
            22 => InvalidChanAssign,
            23 => IncompatibleAssign,
            24 => UnaddressableFieldAssign,
            25 => NotAType,
            26 => InvalidArrayLen,
            27 => BlankIfaceMethod,
            28 => IncomparableMapKey,
            30 => InvalidPtrEmbed,
            31 => BadRecv,
            32 => InvalidRecv,
            33 => DuplicateFieldAndMethod,
            34 => DuplicateMethod,
            35 => InvalidBlank,
            36 => InvalidIota,
            37 => MissingInitBody,
            38 => InvalidInitSig,
            39 => InvalidInitDecl,
            40 => InvalidMainDecl,
            41 => TooManyValues,
            42 => NotAnExpr,
            43 => TruncatedFloat,
            44 => NumericOverflow,
            45 => UndefinedOp,
            46 => MismatchedTypes,
            47 => DivByZero,
            48 => NonNumericIncDec,
            49 => UnaddressableOperand,
            50 => InvalidIndirection,
            51 => NonIndexableOperand,
            52 => InvalidIndex,
            53 => SwappedSliceIndices,
            54 => NonSliceableOperand,
            55 => InvalidSliceExpr,
            56 => InvalidShiftCount,
            57 => InvalidShiftOperand,
            58 => InvalidReceive,
            59 => InvalidSend,
            60 => DuplicateLitKey,
            61 => MissingLitKey,
            62 => InvalidLitIndex,
            63 => OversizeArrayLit,
            64 => MixedStructLit,
            65 => InvalidStructLit,
            66 => MissingLitField,
            67 => DuplicateLitField,
            68 => UnexportedLitField,
            69 => InvalidLitField,
            70 => UntypedLit,
            71 => InvalidLit,
            72 => AmbiguousSelector,
            73 => UndeclaredImportedName,
            74 => UnexportedName,
            75 => UndeclaredName,
            76 => MissingFieldOrMethod,
            77 => BadDotDotDotSyntax,
            78 => NonVariadicDotDotDot,
            81 => InvalidDotDotDot,
            82 => UncalledBuiltin,
            83 => InvalidAppend,
            84 => InvalidCap,
            85 => InvalidClose,
            86 => InvalidCopy,
            87 => InvalidComplex,
            88 => InvalidDelete,
            89 => InvalidImag,
            90 => InvalidLen,
            91 => SwappedMakeArgs,
            92 => InvalidMake,
            93 => InvalidReal,
            94 => InvalidAssert,
            95 => ImpossibleAssert,
            96 => InvalidConversion,
            97 => InvalidUntypedConversion,
            98 => BadOffsetofSyntax,
            99 => InvalidOffsetof,
            100 => UnusedExpr,
            101 => UnusedVar,
            102 => MissingReturn,
            103 => WrongResultCount,
            104 => OutOfScopeResult,
            105 => InvalidCond,
            106 => InvalidPostDecl,
            108 => InvalidIterVar,
            109 => InvalidRangeExpr,
            110 => MisplacedBreak,
            111 => MisplacedContinue,
            112 => MisplacedFallthrough,
            113 => DuplicateCase,
            114 => DuplicateDefault,
            115 => BadTypeKeyword,
            116 => InvalidTypeSwitch,
            117 => InvalidExprSwitch,
            118 => InvalidSelectCase,
            119 => UndeclaredLabel,
            120 => DuplicateLabel,
            121 => MisplacedLabel,
            122 => UnusedLabel,
            123 => JumpOverDecl,
            124 => JumpIntoBlock,
            125 => InvalidMethodExpr,
            126 => WrongArgCount,
            127 => InvalidCall,
            128 => UnusedResults,
            129 => InvalidDefer,
            130 => InvalidGo,
            131 => BadDecl,
            132 => RepeatedDecl,
            133 => InvalidUnsafeAdd,
            134 => InvalidUnsafeSlice,
            135 => UnsupportedFeature,
            136 => NotAGenericType,
            137 => WrongTypeArgCount,
            138 => CannotInferTypeArgs,
            139 => InvalidTypeArg,
            140 => InvalidInstanceCycle,
            141 => InvalidUnion,
            142 => MisplacedConstraintIface,
            143 => InvalidMethodTypeParams,
            144 => MisplacedTypeParam,
            145 => InvalidUnsafeSliceData,
            146 => InvalidUnsafeString,
            148 => InvalidClear,
            149 => TypeTooLarge,
            150 => InvalidMinMaxOperand,
            151 => TooNew,
            _ => return None,
        };
        Some(v)
    }
}
