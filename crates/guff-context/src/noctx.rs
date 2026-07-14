//! Port of [`github.com/sonatard/noctx`](https://github.com/sonatard/noctx).
//!
//! Upstream uses `buildssa`; this port matches the same deny-list via AST
//! call-name resolution (`code::call_name` / `type_func_name`).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;

fn ng_messages() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("net.Listen", "must not be called. use (*net.ListenConfig).Listen"),
            (
                "net.ListenPacket",
                "must not be called. use (*net.ListenConfig).ListenPacket",
            ),
            ("net.Dial", "must not be called. use (*net.Dialer).DialContext"),
            (
                "net.DialTimeout",
                "must not be called. use (*net.Dialer).DialContext with (*net.Dialer).Timeout",
            ),
            (
                "net.LookupCNAME",
                "must not be called. use (*net.Resolver).LookupCNAME with a context",
            ),
            (
                "net.LookupHost",
                "must not be called. use (*net.Resolver).LookupHost with a context",
            ),
            (
                "net.LookupIP",
                "must not be called. use (*net.Resolver).LookupIPAddr with a context",
            ),
            (
                "net.LookupPort",
                "must not be called. use (*net.Resolver).LookupPort with a context",
            ),
            (
                "net.LookupSRV",
                "must not be called. use (*net.Resolver).LookupSRV with a context",
            ),
            (
                "net.LookupMX",
                "must not be called. use (*net.Resolver).LookupMX with a context",
            ),
            (
                "net.LookupNS",
                "must not be called. use (*net.Resolver).LookupNS with a context",
            ),
            (
                "net.LookupTXT",
                "must not be called. use (*net.Resolver).LookupTXT with a context",
            ),
            (
                "net.LookupAddr",
                "must not be called. use (*net.Resolver).LookupAddr with a context",
            ),
            (
                "net/http.Get",
                "must not be called. use net/http.NewRequestWithContext and (*net/http.Client).Do(*http.Request)",
            ),
            (
                "net/http.Head",
                "must not be called. use net/http.NewRequestWithContext and (*net/http.Client).Do(*http.Request)",
            ),
            (
                "net/http.Post",
                "must not be called. use net/http.NewRequestWithContext and (*net/http.Client).Do(*http.Request)",
            ),
            (
                "net/http.PostForm",
                "must not be called. use net/http.NewRequestWithContext and (*net/http.Client).Do(*http.Request)",
            ),
            (
                "(*net/http.Client).Get",
                "must not be called. use (*net/http.Client).Do(*http.Request)",
            ),
            (
                "(*net/http.Client).Head",
                "must not be called. use (*net/http.Client).Do(*http.Request)",
            ),
            (
                "(*net/http.Client).Post",
                "must not be called. use (*net/http.Client).Do(*http.Request)",
            ),
            (
                "(*net/http.Client).PostForm",
                "must not be called. use (*net/http.Client).Do(*http.Request)",
            ),
            (
                "net/http.NewRequest",
                "must not be called. use net/http.NewRequestWithContext",
            ),
            (
                "net/http/httptest.NewRequest",
                "must not be called. use net/http/httptest.NewRequestWithContext",
            ),
            (
                "(*database/sql.DB).Begin",
                "must not be called. use (*database/sql.DB).BeginTx",
            ),
            (
                "(*database/sql.DB).Exec",
                "must not be called. use (*database/sql.DB).ExecContext",
            ),
            (
                "(*database/sql.DB).Ping",
                "must not be called. use (*database/sql.DB).PingContext",
            ),
            (
                "(*database/sql.DB).Prepare",
                "must not be called. use (*database/sql.DB).PrepareContext",
            ),
            (
                "(*database/sql.DB).Query",
                "must not be called. use (*database/sql.DB).QueryContext",
            ),
            (
                "(*database/sql.DB).QueryRow",
                "must not be called. use (*database/sql.DB).QueryRowContext",
            ),
            (
                "(*database/sql.Tx).Exec",
                "must not be called. use (*database/sql.Tx).ExecContext",
            ),
            (
                "(*database/sql.Tx).Prepare",
                "must not be called. use (*database/sql.Tx).PrepareContext",
            ),
            (
                "(*database/sql.Tx).Query",
                "must not be called. use (*database/sql.Tx).QueryContext",
            ),
            (
                "(*database/sql.Tx).QueryRow",
                "must not be called. use (*database/sql.Tx).QueryRowContext",
            ),
            (
                "(*database/sql.Tx).Stmt",
                "must not be called. use (*database/sql.Tx).StmtContext",
            ),
            (
                "(*database/sql.Stmt).Exec",
                "must not be called. use (*database/sql.Conn).ExecContext",
            ),
            (
                "(*database/sql.Stmt).Query",
                "must not be called. use (*database/sql.Conn).QueryContext",
            ),
            (
                "(*database/sql.Stmt).QueryRow",
                "must not be called. use (*database/sql.Conn).QueryRowContext",
            ),
            (
                "os/exec.Command",
                "must not be called. use os/exec.CommandContext",
            ),
            (
                "crypto/tls.Dial",
                "must not be called. use (*crypto/tls.Dialer).DialContext",
            ),
            (
                "crypto/tls.DialWithDialer",
                "must not be called. use (*crypto/tls.Dialer).DialContext with NetDialer",
            ),
            (
                "(*crypto/tls.Conn).Handshake",
                "must not be called. use (*crypto/tls.Conn).HandshakeContext",
            ),
        ])
    })
}

fn full_call_name(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    if let Some(name) = code::call_name(pass, &call.fun) {
        // Prefer method form when available.
        let info = pass.types_info()?;
        let artifacts = pass.pkg().type_artifacts.as_ref()?;
        let obj = match &*call.fun {
            Expr::Ident(id) => info.uses.get(&id.id).copied(),
            Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
            Expr::ParenExpr(p) => match &*p.x {
                Expr::Ident(id) => info.uses.get(&id.id).copied(),
                Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
                _ => None,
            },
            _ => None,
        };
        if let Some(obj_id) = obj {
            if matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
                let methodish = code::type_func_name(
                    &artifacts.types,
                    &artifacts.objects,
                    &artifacts.packages,
                    obj_id,
                );
                if methodish.starts_with('(') {
                    return Some(methodish);
                }
                return Some(name);
            }
        }
        return Some(name);
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "noctx requires inspect analyzer".to_string())?;

    let msgs = ng_messages();
    let mut pending = Vec::new();
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            let NodeRef::CallExpr(call) = n else {
                return true;
            };
            let Some(name) = full_call_name(pass, call) else {
                return true;
            };
            if let Some(msg) = msgs.get(name.as_str()) {
                pending.push((call.lparen.0 as u32, format!("{name} {msg}")));
            }
            true
        });
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "noctx",
        doc: "finds function calls without context.Context",
        url: "https://github.com/sonatard/noctx",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
