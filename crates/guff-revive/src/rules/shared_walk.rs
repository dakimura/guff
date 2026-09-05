//! Shared AST walk for revive rules that only filter nodes (never prune).
//!
//! Enabled checkers ride one [`walk::walk`] per file instead of one walk per rule.

use std::collections::HashMap;

use guff::ast::File;
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use super::{
    argument_limit, atomic, banned_characters, bare_return, bool_literal_in_expr, call_to_gc,
    confusing_naming, confusing_results, constant_logical_expr, context_as_argument, context_keys_type,
    cyclomatic, deep_exit, defer, dot_imports, duplicated_imports, empty_lines, enforce_map_style,
    enforce_repeated_arg_type_style, enforce_slice_style, enforce_switch_style, epoch_naming,
    error_naming, error_return, error_strings, errorf, exported, flag_parameter,
    forbidden_call_in_wg_go, function_length, function_result_limit, get_return, identical_branches,
    identical_ifelseif_branches, identical_ifelseif_conditions, identical_switch_branches,
    identical_switch_conditions, if_return, import_alias_naming, imports_blocklist,
    increment_decrement, inefficient_map_lookup, modifies_parameter, modifies_value_receiver,
    multiline_if_init, nested_structs, optimize_operands_order, package_naming, range,
    range_val_address, range_val_in_closure, receiver_naming, redefines_builtin_id,
    redundant_import_alias, redundant_test_main_exit, string_format, string_of_int, struct_tag,
    time_date, time_equal, time_naming, unchecked_type_assertion, unexported_naming,
    unexported_return, unhandled_error, unnecessary_format, unnecessary_if, unnecessary_stmt,
    unreachable_code, unsecure_url_scheme, unused_parameter, unused_receiver, use_any,
    use_errors_new, use_fmt_print, use_slices_sort, useless_fallthrough, var_naming,
    waitgroup_by_value,
};

/// Fan-out visitor: each enabled rule's checker sees every node; walk never prunes.
struct SharedFileRules<'a> {
    /// Whether the file currently being walked is a `*_test.go`. revive's
    /// `lint.File.IsTest` is a filename check, so it is per file, not per
    /// package, and `run_shared` sets it before each `on_file`.
    file_is_test: bool,
    /// Index of the file currently being walked in `pass.files()`, for rules
    /// that need to reach past the AST into the file's bytes.
    file_index: usize,
    argument_limit: Option<argument_limit::Checker>,
    atomic: Option<atomic::Checker<'a>>,
    banned_characters: Option<banned_characters::Checker>,
    bare_return: Option<bare_return::Checker>,
    bool_literal_in_expr: Option<bool_literal_in_expr::Checker>,
    call_to_gc: Option<call_to_gc::Checker>,
    confusing_naming: Option<confusing_naming::Checker<'a>>,
    confusing_results: Option<confusing_results::Checker>,
    context_as_argument: Option<context_as_argument::Checker<'a>>,
    constant_logical_expr: Option<constant_logical_expr::Checker>,
    context_keys_type: Option<context_keys_type::Checker<'a>>,
    cyclomatic: Option<cyclomatic::Checker>,
    deep_exit: Option<deep_exit::Checker>,
    defer: Option<defer::Checker>,
    dot_imports: Option<dot_imports::Checker>,
    duplicated_imports: Option<duplicated_imports::Checker>,
    empty_lines: Option<empty_lines::Checker<'a>>,
    enforce_map_style: Option<enforce_map_style::Checker>,
    enforce_repeated_arg_type_style: Option<enforce_repeated_arg_type_style::Checker<'a>>,
    enforce_slice_style: Option<enforce_slice_style::Checker>,
    enforce_switch_style: Option<enforce_switch_style::Checker>,
    epoch_naming: Option<epoch_naming::Checker<'a>>,
    error_naming: Option<error_naming::Checker>,
    error_return: Option<error_return::Checker>,
    error_strings: Option<error_strings::Checker>,
    errorf: Option<errorf::Checker<'a>>,
    exported: Option<exported::Checker<'a>>,
    flag_parameter: Option<flag_parameter::Checker>,
    forbidden_call_in_wg_go: Option<forbidden_call_in_wg_go::Checker>,
    function_length: Option<function_length::Checker<'a>>,
    function_result_limit: Option<function_result_limit::Checker>,
    get_return: Option<get_return::Checker>,
    identical_branches: Option<identical_branches::Checker>,
    identical_ifelseif_branches: Option<identical_ifelseif_branches::Checker<'a>>,
    identical_ifelseif_conditions: Option<identical_ifelseif_conditions::Checker<'a>>,
    identical_switch_branches: Option<identical_switch_branches::Checker<'a>>,
    identical_switch_conditions: Option<identical_switch_conditions::Checker<'a>>,
    if_return: Option<if_return::Checker<'a>>,
    import_alias_naming: Option<import_alias_naming::Checker>,
    imports_blocklist: Option<imports_blocklist::Checker>,
    increment_decrement: Option<increment_decrement::Checker>,
    inefficient_map_lookup: Option<inefficient_map_lookup::Checker<'a>>,
    modifies_parameter: Option<modifies_parameter::Checker>,
    modifies_value_receiver: Option<modifies_value_receiver::Checker<'a>>,
    multiline_if_init: Option<multiline_if_init::Checker<'a>>,
    nested_structs: Option<nested_structs::Checker>,
    optimize_operands_order: Option<optimize_operands_order::Checker>,
    package_naming: Option<package_naming::Checker>,
    range: Option<range::Checker>,
    range_val_address: Option<range_val_address::Checker<'a>>,
    range_val_in_closure: Option<range_val_in_closure::Checker>,
    receiver_naming: Option<receiver_naming::Checker>,
    redefines_builtin_id: Option<redefines_builtin_id::Checker>,
    redundant_import_alias: Option<redundant_import_alias::Checker>,
    redundant_test_main_exit: Option<redundant_test_main_exit::Checker>,
    string_format: Option<string_format::Checker>,
    string_of_int: Option<string_of_int::Checker<'a>>,
    struct_tag: Option<struct_tag::Checker>,
    time_date: Option<time_date::Checker>,
    time_equal: Option<time_equal::Checker<'a>>,
    time_naming: Option<time_naming::Checker<'a>>,
    unchecked_type_assertion: Option<unchecked_type_assertion::Checker<'a>>,
    unexported_naming: Option<unexported_naming::Checker>,
    unexported_return: Option<unexported_return::Checker<'a>>,
    unhandled_error: Option<unhandled_error::Checker<'a>>,
    unnecessary_format: Option<unnecessary_format::Checker>,
    unnecessary_if: Option<unnecessary_if::Checker>,
    unnecessary_stmt: Option<unnecessary_stmt::Checker>,
    unreachable_code: Option<unreachable_code::Checker>,
    unsecure_url_scheme: Option<unsecure_url_scheme::Checker>,
    unused_parameter: Option<unused_parameter::Checker>,
    unused_receiver: Option<unused_receiver::Checker>,
    use_any: Option<use_any::Checker>,
    use_errors_new: Option<use_errors_new::Checker>,
    use_fmt_print: Option<use_fmt_print::Checker>,
    use_slices_sort: Option<use_slices_sort::Checker>,
    useless_fallthrough: Option<useless_fallthrough::Checker>,
    var_naming: Option<var_naming::Checker>,
    waitgroup_by_value: Option<waitgroup_by_value::Checker>,
}

macro_rules! take_map {
    ($map:ident, $this:ident, $($name:literal => $field:ident),* $(,)?) => {
        $(
            if let Some(c) = $this.$field.take() {
                $map.insert($name, c.into_failures());
            }
        )*
    };
}

impl<'a> SharedFileRules<'a> {
    fn new(pass: &'a Pass<'a>) -> Self {
        let settings = config::effective_settings(pass);
        let all = config::all_rules();
        let enabled = |name: &str| settings.rule_enabled(name, config::DEFAULT_RULES, all);
        Self {
            file_is_test: false,
            file_index: 0,
            argument_limit: enabled("argument-limit").then(|| argument_limit::Checker::new(pass)),
            atomic: enabled("atomic").then(|| atomic::Checker::try_new(pass)).flatten(),
            banned_characters: enabled("banned-characters")
                .then(|| banned_characters::Checker::try_new(pass))
                .flatten(),
            bare_return: enabled("bare-return").then(bare_return::Checker::new),
            bool_literal_in_expr: enabled("bool-literal-in-expr")
                .then(bool_literal_in_expr::Checker::new),
            call_to_gc: enabled("call-to-gc").then(call_to_gc::Checker::new),
            confusing_naming: enabled("confusing-naming").then(|| confusing_naming::Checker::new(pass)),
            confusing_results: enabled("confusing-results").then(confusing_results::Checker::new),
            context_as_argument: enabled("context-as-argument")
                .then(|| context_as_argument::Checker::new(pass)),
            constant_logical_expr: enabled("constant-logical-expr")
                .then(constant_logical_expr::Checker::new),
            context_keys_type: enabled("context-keys-type")
                .then(|| context_keys_type::Checker::new(pass)),
            cyclomatic: enabled("cyclomatic").then(|| cyclomatic::Checker::new(pass)),
            deep_exit: enabled("deep-exit").then(|| deep_exit::Checker::new(pass)),
            defer: enabled("defer").then(defer::Checker::new),
            dot_imports: enabled("dot-imports").then(dot_imports::Checker::new),
            duplicated_imports: enabled("duplicated-imports").then(duplicated_imports::Checker::new),
            empty_lines: enabled("empty-lines").then(|| empty_lines::Checker::new(pass)),
            enforce_map_style: enabled("enforce-map-style")
                .then(|| enforce_map_style::Checker::try_new(pass))
                .flatten(),
            enforce_repeated_arg_type_style: enabled("enforce-repeated-arg-type-style")
                .then(|| enforce_repeated_arg_type_style::Checker::try_new(pass))
                .flatten(),
            enforce_slice_style: enabled("enforce-slice-style")
                .then(|| enforce_slice_style::Checker::try_new(pass))
                .flatten(),
            enforce_switch_style: enabled("enforce-switch-style")
                .then(|| enforce_switch_style::Checker::new(pass)),
            epoch_naming: enabled("epoch-naming").then(|| epoch_naming::Checker::new(pass)),
            error_naming: enabled("error-naming").then(error_naming::Checker::new),
            error_return: enabled("error-return").then(error_return::Checker::new),
            error_strings: enabled("error-strings").then(error_strings::Checker::new),
            errorf: enabled("errorf").then(|| errorf::Checker::new(pass)),
            exported: enabled("exported")
                .then(|| exported::Checker::try_new(pass))
                .flatten(),
            flag_parameter: enabled("flag-parameter").then(flag_parameter::Checker::new),
            forbidden_call_in_wg_go: enabled("forbidden-call-in-wg-go")
                .then(|| forbidden_call_in_wg_go::Checker::try_new(pass))
                .flatten(),
            function_length: enabled("function-length")
                .then(|| function_length::Checker::new(pass)),
            function_result_limit: enabled("function-result-limit")
                .then(|| function_result_limit::Checker::new(pass)),
            get_return: enabled("get-return").then(get_return::Checker::new),
            identical_branches: enabled("identical-branches").then(identical_branches::Checker::new),
            identical_ifelseif_branches: enabled("identical-ifelseif-branches")
                .then(|| identical_ifelseif_branches::Checker::new(pass)),
            identical_ifelseif_conditions: enabled("identical-ifelseif-conditions")
                .then(|| identical_ifelseif_conditions::Checker::new(pass)),
            identical_switch_branches: enabled("identical-switch-branches")
                .then(|| identical_switch_branches::Checker::new(pass)),
            identical_switch_conditions: enabled("identical-switch-conditions")
                .then(|| identical_switch_conditions::Checker::new(pass)),
            if_return: enabled("if-return").then(|| if_return::Checker::new(pass)),
            import_alias_naming: enabled("import-alias-naming").then(|| import_alias_naming::Checker::new(pass)),
            imports_blocklist: enabled("imports-blocklist")
                .then(|| imports_blocklist::Checker::try_new(pass))
                .flatten(),
            increment_decrement: enabled("increment-decrement")
                .then(increment_decrement::Checker::new),
            inefficient_map_lookup: enabled("inefficient-map-lookup")
                .then(|| inefficient_map_lookup::Checker::new(pass)),
            modifies_parameter: enabled("modifies-parameter")
                .then(modifies_parameter::Checker::new),
            modifies_value_receiver: enabled("modifies-value-receiver")
                .then(|| modifies_value_receiver::Checker::new(pass)),
            multiline_if_init: enabled("multiline-if-init")
                .then(|| multiline_if_init::Checker::new(pass)),
            nested_structs: enabled("nested-structs").then(nested_structs::Checker::new),
            optimize_operands_order: enabled("optimize-operands-order")
                .then(optimize_operands_order::Checker::new),
            package_naming: enabled("package-naming").then(package_naming::Checker::new),
            range: enabled("range").then(range::Checker::new),
            range_val_address: (enabled("range-val-address") && range_val_address::applies(pass))
                .then(|| range_val_address::Checker::new(pass)),
            range_val_in_closure: (enabled("range-val-in-closure")
                && range_val_in_closure::applies(pass))
            .then(range_val_in_closure::Checker::new),
            receiver_naming: enabled("receiver-naming").then(receiver_naming::Checker::new),
            redefines_builtin_id: enabled("redefines-builtin-id")
                .then(redefines_builtin_id::Checker::new),
            redundant_import_alias: enabled("redundant-import-alias")
                .then(redundant_import_alias::Checker::new),
            redundant_test_main_exit: enabled("redundant-test-main-exit")
                .then(|| redundant_test_main_exit::Checker::try_new(pass))
                .flatten(),
            string_format: enabled("string-format")
                .then(|| string_format::Checker::try_new(pass))
                .flatten(),
            string_of_int: enabled("string-of-int").then(|| string_of_int::Checker::new(pass)),
            struct_tag: enabled("struct-tag").then(struct_tag::Checker::new),
            time_date: enabled("time-date").then(time_date::Checker::new),
            time_equal: enabled("time-equal").then(|| time_equal::Checker::new(pass)),
            time_naming: enabled("time-naming").then(|| time_naming::Checker::new(pass)),
            unchecked_type_assertion: enabled("unchecked-type-assertion")
                .then(|| unchecked_type_assertion::Checker::new(pass)),
            unexported_naming: enabled("unexported-naming").then(unexported_naming::Checker::new),
            unexported_return: enabled("unexported-return")
                .then(|| unexported_return::Checker::try_new(pass))
                .flatten(),
            unhandled_error: enabled("unhandled-error")
                .then(|| unhandled_error::Checker::try_new(pass))
                .flatten(),
            unnecessary_format: enabled("unnecessary-format").then(unnecessary_format::Checker::new),
            unnecessary_if: enabled("unnecessary-if").then(unnecessary_if::Checker::new),
            unnecessary_stmt: enabled("unnecessary-stmt").then(unnecessary_stmt::Checker::new),
            unreachable_code: enabled("unreachable-code").then(unreachable_code::Checker::new),
            unsecure_url_scheme: enabled("unsecure-url-scheme")
                .then(|| unsecure_url_scheme::Checker::try_new(pass))
                .flatten(),
            unused_parameter: enabled("unused-parameter")
                .then(|| unused_parameter::Checker::new(pass)),
            unused_receiver: enabled("unused-receiver")
                .then(|| unused_receiver::Checker::new(pass)),
            use_any: enabled("use-any").then(use_any::Checker::new),
            use_errors_new: enabled("use-errors-new").then(use_errors_new::Checker::new),
            use_fmt_print: enabled("use-fmt-print").then(|| use_fmt_print::Checker::new(pass)),
            use_slices_sort: enabled("use-slices-sort").then(use_slices_sort::Checker::new),
            useless_fallthrough: enabled("useless-fallthrough")
                .then(useless_fallthrough::Checker::new),
            var_naming: enabled("var-naming").then(|| var_naming::Checker::new(pass)),
            waitgroup_by_value: enabled("waitgroup-by-value").then(waitgroup_by_value::Checker::new),
        }
    }

    fn any_enabled(&self) -> bool {
        macro_rules! any {
            ($($f:ident),* $(,)?) => { $(self.$f.is_some())||* };
        }
        any!(
            argument_limit,
            atomic,
            banned_characters,
            bare_return,
            bool_literal_in_expr,
            call_to_gc,
            confusing_naming,
            confusing_results,
            context_as_argument,
            constant_logical_expr,
            context_keys_type,
            cyclomatic,
            deep_exit,
            defer,
            dot_imports,
            duplicated_imports,
            empty_lines,
            enforce_map_style,
            enforce_repeated_arg_type_style,
            enforce_slice_style,
            enforce_switch_style,
            epoch_naming,
            error_naming,
            error_return,
            error_strings,
            errorf,
            exported,
            flag_parameter,
            forbidden_call_in_wg_go,
            function_length,
            function_result_limit,
            get_return,
            identical_branches,
            identical_ifelseif_branches,
            identical_ifelseif_conditions,
            identical_switch_branches,
            identical_switch_conditions,
            if_return,
            import_alias_naming,
            imports_blocklist,
            increment_decrement,
            inefficient_map_lookup,
            modifies_parameter,
            modifies_value_receiver,
            multiline_if_init,
            nested_structs,
            optimize_operands_order,
            package_naming,
            range,
            range_val_address,
            range_val_in_closure,
            receiver_naming,
            redefines_builtin_id,
            redundant_import_alias,
            redundant_test_main_exit,
            string_format,
            string_of_int,
            struct_tag,
            time_date,
            time_equal,
            time_naming,
            unchecked_type_assertion,
            unexported_naming,
            unexported_return,
            unhandled_error,
            unnecessary_format,
            unnecessary_if,
            unnecessary_stmt,
            unreachable_code,
            unsecure_url_scheme,
            unused_parameter,
            unused_receiver,
            use_any,
            use_errors_new,
            use_fmt_print,
            use_slices_sort,
            useless_fallthrough,
            var_naming,
            waitgroup_by_value,
        )
    }

    fn on_file(&mut self, file: &'a File) {
        if let Some(c) = &mut self.if_return {
            c.on_file(self.file_index);
        }
        if let Some(c) = &mut self.duplicated_imports {
            c.on_file(file);
        }
        if let Some(c) = &mut self.empty_lines {
            c.on_file(file);
        }
        if let Some(c) = &mut self.exported {
            c.on_file(file);
        }
        if let Some(c) = &mut self.function_length {
            c.on_file(file);
        }
        if let Some(c) = &mut self.receiver_naming {
            c.on_file(file);
        }
        if let Some(c) = &mut self.unexported_naming {
            c.on_file(file);
        }
        if let Some(c) = &mut self.unsecure_url_scheme {
            c.on_file(self.file_is_test);
        }
        if let Some(c) = &mut self.deep_exit {
            c.on_file(self.file_is_test);
        }
        if let Some(c) = &mut self.redundant_test_main_exit {
            c.on_file(self.file_is_test);
        }
        if let Some(c) = &mut self.unnecessary_stmt {
            c.on_file(file);
        }
    }

    fn into_map(mut self) -> HashMap<&'static str, Vec<Failure>> {
        let mut map = HashMap::new();
        take_map!(
            map,
            self,
            "argument-limit" => argument_limit,
            "atomic" => atomic,
            "banned-characters" => banned_characters,
            "bare-return" => bare_return,
            "bool-literal-in-expr" => bool_literal_in_expr,
            "call-to-gc" => call_to_gc,
            "confusing-naming" => confusing_naming,
            "confusing-results" => confusing_results,
            "context-as-argument" => context_as_argument,
            "constant-logical-expr" => constant_logical_expr,
            "context-keys-type" => context_keys_type,
            "cyclomatic" => cyclomatic,
            "deep-exit" => deep_exit,
            "defer" => defer,
            "dot-imports" => dot_imports,
            "duplicated-imports" => duplicated_imports,
            "empty-lines" => empty_lines,
            "enforce-map-style" => enforce_map_style,
            "enforce-repeated-arg-type-style" => enforce_repeated_arg_type_style,
            "enforce-slice-style" => enforce_slice_style,
            "enforce-switch-style" => enforce_switch_style,
            "epoch-naming" => epoch_naming,
            "error-naming" => error_naming,
            "error-return" => error_return,
            "error-strings" => error_strings,
            "errorf" => errorf,
            "exported" => exported,
            "flag-parameter" => flag_parameter,
            "forbidden-call-in-wg-go" => forbidden_call_in_wg_go,
            "function-length" => function_length,
            "function-result-limit" => function_result_limit,
            "get-return" => get_return,
            "identical-branches" => identical_branches,
            "identical-ifelseif-branches" => identical_ifelseif_branches,
            "identical-ifelseif-conditions" => identical_ifelseif_conditions,
            "identical-switch-branches" => identical_switch_branches,
            "identical-switch-conditions" => identical_switch_conditions,
            "if-return" => if_return,
            "import-alias-naming" => import_alias_naming,
            "imports-blocklist" => imports_blocklist,
            "increment-decrement" => increment_decrement,
            "inefficient-map-lookup" => inefficient_map_lookup,
            "modifies-parameter" => modifies_parameter,
            "modifies-value-receiver" => modifies_value_receiver,
            "multiline-if-init" => multiline_if_init,
            "nested-structs" => nested_structs,
            "optimize-operands-order" => optimize_operands_order,
            "package-naming" => package_naming,
            "range" => range,
            "range-val-address" => range_val_address,
            "range-val-in-closure" => range_val_in_closure,
            "receiver-naming" => receiver_naming,
            "redefines-builtin-id" => redefines_builtin_id,
            "redundant-import-alias" => redundant_import_alias,
            "redundant-test-main-exit" => redundant_test_main_exit,
            "string-format" => string_format,
            "string-of-int" => string_of_int,
            "struct-tag" => struct_tag,
            "time-date" => time_date,
            "time-equal" => time_equal,
            "time-naming" => time_naming,
            "unchecked-type-assertion" => unchecked_type_assertion,
            "unexported-naming" => unexported_naming,
            "unexported-return" => unexported_return,
            "unhandled-error" => unhandled_error,
            "unnecessary-format" => unnecessary_format,
            "unnecessary-if" => unnecessary_if,
            "unnecessary-stmt" => unnecessary_stmt,
            "unreachable-code" => unreachable_code,
            "unsecure-url-scheme" => unsecure_url_scheme,
            "unused-parameter" => unused_parameter,
            "unused-receiver" => unused_receiver,
            "use-any" => use_any,
            "use-errors-new" => use_errors_new,
            "use-fmt-print" => use_fmt_print,
            "use-slices-sort" => use_slices_sort,
            "useless-fallthrough" => useless_fallthrough,
            "var-naming" => var_naming,
            "waitgroup-by-value" => waitgroup_by_value,
        );
        map
    }
}

impl<'a> Visitor<'a> for SharedFileRules<'a> {
    fn enter(&mut self, n: NodeRef<'a>) -> bool {
        macro_rules! visit_all {
            ($($f:ident),* $(,)?) => {
                $(
                    if let Some(c) = &mut self.$f {
                        c.visit(n);
                    }
                )*
            };
        }
        visit_all!(
            argument_limit,
            atomic,
            banned_characters,
            bare_return,
            bool_literal_in_expr,
            call_to_gc,
            confusing_naming,
            confusing_results,
            context_as_argument,
            constant_logical_expr,
            context_keys_type,
            cyclomatic,
            deep_exit,
            defer,
            dot_imports,
            duplicated_imports,
            empty_lines,
            enforce_map_style,
            enforce_repeated_arg_type_style,
            enforce_slice_style,
            enforce_switch_style,
            epoch_naming,
            error_naming,
            error_return,
            error_strings,
            errorf,
            exported,
            flag_parameter,
            forbidden_call_in_wg_go,
            function_length,
            function_result_limit,
            get_return,
            identical_branches,
            identical_ifelseif_branches,
            identical_ifelseif_conditions,
            identical_switch_branches,
            identical_switch_conditions,
            if_return,
            import_alias_naming,
            imports_blocklist,
            increment_decrement,
            inefficient_map_lookup,
            modifies_parameter,
            modifies_value_receiver,
            multiline_if_init,
            nested_structs,
            optimize_operands_order,
            package_naming,
            range,
            range_val_address,
            range_val_in_closure,
            receiver_naming,
            redefines_builtin_id,
            redundant_import_alias,
            redundant_test_main_exit,
            string_format,
            string_of_int,
            struct_tag,
            time_date,
            time_equal,
            time_naming,
            unchecked_type_assertion,
            unexported_naming,
            unexported_return,
            unhandled_error,
            unnecessary_format,
            unnecessary_if,
            unnecessary_stmt,
            unreachable_code,
            unsecure_url_scheme,
            unused_parameter,
            unused_receiver,
            use_any,
            use_errors_new,
            use_fmt_print,
            use_slices_sort,
            useless_fallthrough,
            var_naming,
            waitgroup_by_value,
        );
        true
    }
}

/// Run all shared-walk rules once per file; returns failures keyed by rule name.
pub fn run_shared(pass: &Pass<'_>) -> HashMap<&'static str, Vec<Failure>> {
    let mut shared = SharedFileRules::new(pass);
    if !shared.any_enabled() {
        return HashMap::new();
    }
    for (index, file) in pass.files().iter().enumerate() {
        shared.file_is_test = crate::util::file_is_test(pass, file);
        shared.file_index = index;
        shared.on_file(file);
        walk::walk(&mut shared, NodeRef::File(file));
    }
    shared.into_map()
}
