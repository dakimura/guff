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
    atomic, banned_characters, bare_return, bool_literal_in_expr, call_to_gc,
    constant_logical_expr, context_keys_type, empty_lines, enforce_map_style, enforce_slice_style,
    enforce_switch_style, epoch_naming, error_strings, errorf, identical_branches,
    identical_ifelseif_branches, identical_ifelseif_conditions, identical_switch_branches,
    identical_switch_conditions, if_return, increment_decrement, multiline_if_init, nested_structs,
    optimize_operands_order, range, range_val_in_closure, redefines_builtin_id, string_format,
    string_of_int, struct_tag, time_date, time_equal, unchecked_type_assertion, unexported_naming,
    unhandled_error, unnecessary_format, unnecessary_if, unnecessary_stmt, unreachable_code,
    unsecure_url_scheme, unused_parameter, use_any, use_errors_new, use_fmt_print, use_slices_sort,
    useless_fallthrough, var_declaration, var_naming,
};

/// Fan-out visitor: each enabled rule's checker sees every node; walk never prunes.
struct SharedFileRules<'a> {
    atomic: Option<atomic::Checker<'a>>,
    banned_characters: Option<banned_characters::Checker>,
    bare_return: Option<bare_return::Checker>,
    bool_literal_in_expr: Option<bool_literal_in_expr::Checker>,
    call_to_gc: Option<call_to_gc::Checker>,
    constant_logical_expr: Option<constant_logical_expr::Checker>,
    context_keys_type: Option<context_keys_type::Checker<'a>>,
    empty_lines: Option<empty_lines::Checker<'a>>,
    enforce_map_style: Option<enforce_map_style::Checker>,
    enforce_slice_style: Option<enforce_slice_style::Checker>,
    enforce_switch_style: Option<enforce_switch_style::Checker>,
    epoch_naming: Option<epoch_naming::Checker<'a>>,
    error_strings: Option<error_strings::Checker>,
    errorf: Option<errorf::Checker<'a>>,
    identical_branches: Option<identical_branches::Checker>,
    identical_ifelseif_branches: Option<identical_ifelseif_branches::Checker<'a>>,
    identical_ifelseif_conditions: Option<identical_ifelseif_conditions::Checker<'a>>,
    identical_switch_branches: Option<identical_switch_branches::Checker<'a>>,
    identical_switch_conditions: Option<identical_switch_conditions::Checker<'a>>,
    if_return: Option<if_return::Checker>,
    increment_decrement: Option<increment_decrement::Checker>,
    multiline_if_init: Option<multiline_if_init::Checker<'a>>,
    nested_structs: Option<nested_structs::Checker>,
    optimize_operands_order: Option<optimize_operands_order::Checker>,
    range: Option<range::Checker>,
    range_val_in_closure: Option<range_val_in_closure::Checker>,
    redefines_builtin_id: Option<redefines_builtin_id::Checker>,
    string_format: Option<string_format::Checker>,
    string_of_int: Option<string_of_int::Checker<'a>>,
    struct_tag: Option<struct_tag::Checker>,
    time_date: Option<time_date::Checker>,
    time_equal: Option<time_equal::Checker<'a>>,
    unchecked_type_assertion: Option<unchecked_type_assertion::Checker>,
    unexported_naming: Option<unexported_naming::Checker>,
    unhandled_error: Option<unhandled_error::Checker<'a>>,
    unnecessary_format: Option<unnecessary_format::Checker>,
    unnecessary_if: Option<unnecessary_if::Checker>,
    unnecessary_stmt: Option<unnecessary_stmt::Checker>,
    unreachable_code: Option<unreachable_code::Checker>,
    unsecure_url_scheme: Option<unsecure_url_scheme::Checker>,
    unused_parameter: Option<unused_parameter::Checker>,
    use_any: Option<use_any::Checker>,
    use_errors_new: Option<use_errors_new::Checker>,
    use_fmt_print: Option<use_fmt_print::Checker>,
    use_slices_sort: Option<use_slices_sort::Checker>,
    useless_fallthrough: Option<useless_fallthrough::Checker>,
    var_declaration: Option<var_declaration::Checker<'a>>,
    var_naming: Option<var_naming::Checker>,
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
            atomic: enabled("atomic").then(|| atomic::Checker::try_new(pass)).flatten(),
            banned_characters: enabled("banned-characters")
                .then(|| banned_characters::Checker::try_new(pass))
                .flatten(),
            bare_return: enabled("bare-return").then(bare_return::Checker::new),
            bool_literal_in_expr: enabled("bool-literal-in-expr")
                .then(bool_literal_in_expr::Checker::new),
            call_to_gc: enabled("call-to-gc").then(call_to_gc::Checker::new),
            constant_logical_expr: enabled("constant-logical-expr")
                .then(constant_logical_expr::Checker::new),
            context_keys_type: enabled("context-keys-type")
                .then(|| context_keys_type::Checker::new(pass)),
            empty_lines: enabled("empty-lines").then(|| empty_lines::Checker::new(pass)),
            enforce_map_style: enabled("enforce-map-style")
                .then(|| enforce_map_style::Checker::try_new(pass))
                .flatten(),
            enforce_slice_style: enabled("enforce-slice-style")
                .then(|| enforce_slice_style::Checker::try_new(pass))
                .flatten(),
            enforce_switch_style: enabled("enforce-switch-style")
                .then(|| enforce_switch_style::Checker::new(pass)),
            epoch_naming: enabled("epoch-naming").then(|| epoch_naming::Checker::new(pass)),
            error_strings: enabled("error-strings").then(error_strings::Checker::new),
            errorf: enabled("errorf").then(|| errorf::Checker::new(pass)),
            identical_branches: enabled("identical-branches").then(identical_branches::Checker::new),
            identical_ifelseif_branches: enabled("identical-ifelseif-branches")
                .then(|| identical_ifelseif_branches::Checker::new(pass)),
            identical_ifelseif_conditions: enabled("identical-ifelseif-conditions")
                .then(|| identical_ifelseif_conditions::Checker::new(pass)),
            identical_switch_branches: enabled("identical-switch-branches")
                .then(|| identical_switch_branches::Checker::new(pass)),
            identical_switch_conditions: enabled("identical-switch-conditions")
                .then(|| identical_switch_conditions::Checker::new(pass)),
            if_return: enabled("if-return").then(if_return::Checker::new),
            increment_decrement: enabled("increment-decrement")
                .then(increment_decrement::Checker::new),
            multiline_if_init: enabled("multiline-if-init")
                .then(|| multiline_if_init::Checker::new(pass)),
            nested_structs: enabled("nested-structs").then(nested_structs::Checker::new),
            optimize_operands_order: enabled("optimize-operands-order")
                .then(optimize_operands_order::Checker::new),
            range: enabled("range").then(range::Checker::new),
            range_val_in_closure: enabled("range-val-in-closure")
                .then(range_val_in_closure::Checker::new),
            redefines_builtin_id: enabled("redefines-builtin-id")
                .then(redefines_builtin_id::Checker::new),
            string_format: enabled("string-format")
                .then(|| string_format::Checker::try_new(pass))
                .flatten(),
            string_of_int: enabled("string-of-int").then(|| string_of_int::Checker::new(pass)),
            struct_tag: enabled("struct-tag").then(struct_tag::Checker::new),
            time_date: enabled("time-date").then(time_date::Checker::new),
            time_equal: enabled("time-equal").then(|| time_equal::Checker::new(pass)),
            unchecked_type_assertion: enabled("unchecked-type-assertion")
                .then(unchecked_type_assertion::Checker::new),
            unexported_naming: enabled("unexported-naming").then(unexported_naming::Checker::new),
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
            unused_parameter: enabled("unused-parameter").then(unused_parameter::Checker::new),
            use_any: enabled("use-any").then(use_any::Checker::new),
            use_errors_new: enabled("use-errors-new").then(use_errors_new::Checker::new),
            use_fmt_print: enabled("use-fmt-print").then(|| use_fmt_print::Checker::new(pass)),
            use_slices_sort: enabled("use-slices-sort").then(use_slices_sort::Checker::new),
            useless_fallthrough: enabled("useless-fallthrough")
                .then(useless_fallthrough::Checker::new),
            var_declaration: enabled("var-declaration")
                .then(|| var_declaration::Checker::new(pass)),
            var_naming: enabled("var-naming").then(|| var_naming::Checker::new(pass)),
        }
    }

    fn any_enabled(&self) -> bool {
        macro_rules! any {
            ($($f:ident),* $(,)?) => { $(self.$f.is_some())||* };
        }
        any!(
            atomic,
            banned_characters,
            bare_return,
            bool_literal_in_expr,
            call_to_gc,
            constant_logical_expr,
            context_keys_type,
            empty_lines,
            enforce_map_style,
            enforce_slice_style,
            enforce_switch_style,
            epoch_naming,
            error_strings,
            errorf,
            identical_branches,
            identical_ifelseif_branches,
            identical_ifelseif_conditions,
            identical_switch_branches,
            identical_switch_conditions,
            if_return,
            increment_decrement,
            multiline_if_init,
            nested_structs,
            optimize_operands_order,
            range,
            range_val_in_closure,
            redefines_builtin_id,
            string_format,
            string_of_int,
            struct_tag,
            time_date,
            time_equal,
            unchecked_type_assertion,
            unexported_naming,
            unhandled_error,
            unnecessary_format,
            unnecessary_if,
            unnecessary_stmt,
            unreachable_code,
            unsecure_url_scheme,
            unused_parameter,
            use_any,
            use_errors_new,
            use_fmt_print,
            use_slices_sort,
            useless_fallthrough,
            var_declaration,
            var_naming,
        )
    }

    fn on_file(&mut self, file: &'a File) {
        if let Some(c) = &mut self.empty_lines {
            c.on_file(file);
        }
        if let Some(c) = &mut self.unexported_naming {
            c.on_file(file);
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
            "atomic" => atomic,
            "banned-characters" => banned_characters,
            "bare-return" => bare_return,
            "bool-literal-in-expr" => bool_literal_in_expr,
            "call-to-gc" => call_to_gc,
            "constant-logical-expr" => constant_logical_expr,
            "context-keys-type" => context_keys_type,
            "empty-lines" => empty_lines,
            "enforce-map-style" => enforce_map_style,
            "enforce-slice-style" => enforce_slice_style,
            "enforce-switch-style" => enforce_switch_style,
            "epoch-naming" => epoch_naming,
            "error-strings" => error_strings,
            "errorf" => errorf,
            "identical-branches" => identical_branches,
            "identical-ifelseif-branches" => identical_ifelseif_branches,
            "identical-ifelseif-conditions" => identical_ifelseif_conditions,
            "identical-switch-branches" => identical_switch_branches,
            "identical-switch-conditions" => identical_switch_conditions,
            "if-return" => if_return,
            "increment-decrement" => increment_decrement,
            "multiline-if-init" => multiline_if_init,
            "nested-structs" => nested_structs,
            "optimize-operands-order" => optimize_operands_order,
            "range" => range,
            "range-val-in-closure" => range_val_in_closure,
            "redefines-builtin-id" => redefines_builtin_id,
            "string-format" => string_format,
            "string-of-int" => string_of_int,
            "struct-tag" => struct_tag,
            "time-date" => time_date,
            "time-equal" => time_equal,
            "unchecked-type-assertion" => unchecked_type_assertion,
            "unexported-naming" => unexported_naming,
            "unhandled-error" => unhandled_error,
            "unnecessary-format" => unnecessary_format,
            "unnecessary-if" => unnecessary_if,
            "unnecessary-stmt" => unnecessary_stmt,
            "unreachable-code" => unreachable_code,
            "unsecure-url-scheme" => unsecure_url_scheme,
            "unused-parameter" => unused_parameter,
            "use-any" => use_any,
            "use-errors-new" => use_errors_new,
            "use-fmt-print" => use_fmt_print,
            "use-slices-sort" => use_slices_sort,
            "useless-fallthrough" => useless_fallthrough,
            "var-declaration" => var_declaration,
            "var-naming" => var_naming,
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
            atomic,
            banned_characters,
            bare_return,
            bool_literal_in_expr,
            call_to_gc,
            constant_logical_expr,
            context_keys_type,
            empty_lines,
            enforce_map_style,
            enforce_slice_style,
            enforce_switch_style,
            epoch_naming,
            error_strings,
            errorf,
            identical_branches,
            identical_ifelseif_branches,
            identical_ifelseif_conditions,
            identical_switch_branches,
            identical_switch_conditions,
            if_return,
            increment_decrement,
            multiline_if_init,
            nested_structs,
            optimize_operands_order,
            range,
            range_val_in_closure,
            redefines_builtin_id,
            string_format,
            string_of_int,
            struct_tag,
            time_date,
            time_equal,
            unchecked_type_assertion,
            unexported_naming,
            unhandled_error,
            unnecessary_format,
            unnecessary_if,
            unnecessary_stmt,
            unreachable_code,
            unsecure_url_scheme,
            unused_parameter,
            use_any,
            use_errors_new,
            use_fmt_print,
            use_slices_sort,
            useless_fallthrough,
            var_declaration,
            var_naming,
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
    for file in pass.files() {
        shared.on_file(file);
        walk::walk(&mut shared, NodeRef::File(file));
    }
    shared.into_map()
}
