// `script::attempt(|| e)` is dropped in `.bs`: `e` runs as it is, and a
// failure ends the engine call as any other does.

use crate::cx::Cx;
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;

impl Cx<'_> {
    /// The closure body of `script::attempt(|| body)`.
    pub(crate) fn attempted<'e>(&self, e: &'e ast::Expr) -> Option<&'e ast::Expr> {
        let ast::Expr::Call(c) = e else { return None };
        if self.text(c.expr.span()).trim() != "script::attempt" || c.args.len() != 1 {
            return None;
        }
        match c.args.first() {
            Some((ast::Expr::Closure(cl), _)) => Some(&cl.body),
            _ => None,
        }
    }

    /// `let (ok, v) = script::attempt(|| e);` as `let v = e; let ok = true;`.
    pub(crate) fn attempt_let(&mut self, t: &ast::PatTuple, e: &ast::Expr) -> String {
        let body = self.attempted(e).expect("the caller checked");
        self.note(
            e.span(),
            "attempt",
            "`script::attempt` dropped: the call runs as it is",
        );
        let ex = self.expr(body);
        let names: Vec<Option<String>> = t
            .items
            .iter()
            .map(|(p, _)| match p {
                ast::Pat::Path(pp) => Some(self.text(pp.path.span()).trim().to_string()),
                _ => None,
            })
            .collect();
        let mut lines = vec![];
        let value = names.get(1).cloned().flatten();
        match (&ex.t, value) {
            (Ty::Void | Ty::Unknown | Ty::Nil, Some(v)) => {
                lines.push(format!("{};", ex.s));
                self.learn_local(&v, &Ty::Str);
                let out = self.bind(&v, false);
                lines.push(format!("let {out} = \"\";"));
            }
            (_, Some(v)) => {
                self.learn_local(&v, &ex.t);
                let out = self.bind(&v, false);
                lines.push(format!("let {out} = {};", ex.s));
            }
            (_, None) => lines.push(format!("{};", ex.s)),
        }
        if let Some(Some(ok)) = names.first() {
            self.learn_local(ok, &Ty::Bool);
            let out = self.bind(ok, false);
            lines.push(format!("let {out} = true;"));
        }
        lines.join("\n")
    }
}
