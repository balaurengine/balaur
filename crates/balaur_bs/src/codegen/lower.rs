// An `async fn` becomes a block graph that a `br_table` dispatcher runs;
// every `await` ends a block, so resuming is jumping to the next one.

use super::func::{R, err};
use super::{Gen, Kind};
use crate::ast::{BinOp, Block, Expr, ExprKind, Pos, Stmt, StmtKind, Ty};

#[derive(Clone)]
pub(crate) enum Term {
    Open,
    Goto(usize),
    Branch(Expr, usize, usize),
    /// A host async call; `next` starts by reading what it resolved to.
    AwaitHost {
        call: Expr,
        site: u32,
        next: usize,
    },
    /// Allocate the callee's frame and start it, then poll it in `poll`.
    AwaitStart {
        call: Expr,
        site: u32,
        poll: usize,
    },
    AwaitPoll {
        key: String,
        with_self: bool,
        site: u32,
        next: usize,
    },
    Done,
}

pub(crate) struct CfgBlock {
    pub stmts: Vec<Stmt>,
    pub term: Term,
}

pub(crate) struct Cfg<'a> {
    pub blocks: Vec<CfgBlock>,
    loops: Vec<(usize, usize)>,
    tmp: u32,
    sites: u32,
    g: &'a Gen,
    owner: Option<String>,
}

fn block_awaits(b: &Block) -> bool {
    b.stmts.iter().any(awaits) || b.tail.as_ref().is_some_and(|t| expr_awaits(t))
}
fn expr_awaits(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Await(_) => true,
        ExprKind::If(c, t, el) => {
            expr_awaits(c) || block_awaits(t) || el.as_ref().is_some_and(block_awaits)
        }
        _ => false,
    }
}
pub(crate) fn awaits(s: &Stmt) -> bool {
    match &s.kind {
        StmtKind::Let { init: e, .. } | StmtKind::Assign { value: e, .. } | StmtKind::Expr(e) => {
            expr_awaits(e)
        }
        StmtKind::While { body, .. }
        | StmtKind::Loop(body)
        | StmtKind::ForRange { body, .. }
        | StmtKind::ForEach { body, .. } => block_awaits(body),
        StmtKind::Return(v) => v.as_ref().is_some_and(expr_awaits),
        StmtKind::Break | StmtKind::Continue => false,
    }
}
// A break or continue that leaves this statement for a loop outside it.
fn escapes(s: &Stmt, depth: u32) -> bool {
    match &s.kind {
        StmtKind::Break | StmtKind::Continue => depth == 0,
        StmtKind::While { body, .. }
        | StmtKind::Loop(body)
        | StmtKind::ForRange { body, .. }
        | StmtKind::ForEach { body, .. } => body.stmts.iter().any(|s| escapes(s, depth + 1)),
        StmtKind::Expr(Expr {
            kind: ExprKind::If(_, t, e),
            ..
        }) => {
            t.stmts.iter().any(|s| escapes(s, depth))
                || e.as_ref()
                    .is_some_and(|e| e.stmts.iter().any(|s| escapes(s, depth)))
        }
        _ => false,
    }
}

fn var(name: &str, pos: Pos) -> Expr {
    Expr {
        kind: ExprKind::Var(name.to_string()),
        pos,
    }
}
fn stmt(kind: StmtKind, pos: Pos) -> Stmt {
    Stmt { kind, pos }
}
fn plus_one(name: &str, pos: Pos) -> Stmt {
    stmt(
        StmtKind::Assign {
            target: var(name, pos),
            op: Some(BinOp::Add),
            value: Expr {
                kind: ExprKind::Int(1),
                pos,
            },
        },
        pos,
    )
}

impl<'a> Cfg<'a> {
    pub(crate) fn new(g: &'a Gen, owner: Option<String>) -> Self {
        Cfg {
            blocks: vec![],
            loops: vec![],
            tmp: 0,
            sites: 0,
            g,
            owner,
        }
    }
    pub(crate) fn new_block(&mut self) -> usize {
        self.blocks.push(CfgBlock {
            stmts: vec![],
            term: Term::Open,
        });
        self.blocks.len() - 1
    }
    pub(crate) fn seal(&mut self, b: usize, t: Term) {
        if matches!(self.blocks[b].term, Term::Open) {
            self.blocks[b].term = t;
        }
    }

    // The awaited call's result type, and the script fn that runs it if the host does not.
    fn awaited(&self, call: &Expr) -> R<(Ty, Option<(String, bool)>)> {
        match &call.kind {
            ExprKind::Call(name, _) => match self.g.sigs.get(name) {
                Some(s) if matches!(s.kind, Kind::ExternAsync | Kind::HostAsync) => {
                    Ok((s.ret.clone(), None))
                }
                Some(s) if s.kind == Kind::Async && s.owner.is_none() => {
                    Ok((s.ret.clone(), Some((name.clone(), false))))
                }
                _ => err(call.pos, format!("`{name}` is not an async function")),
            },
            ExprKind::Method(base, name, _) if matches!(base.kind, ExprKind::SelfRef) => {
                let owner = self.owner.clone().ok_or(crate::Error {
                    pos: call.pos,
                    msg: "`self` outside an impl".into(),
                })?;
                let key = format!("{owner}.{name}");
                match self.g.sigs.get(&key) {
                    Some(s) if s.kind == Kind::Async => Ok((s.ret.clone(), Some((key, true)))),
                    _ => err(call.pos, format!("`{key}` is not an async method")),
                }
            }
            _ => err(
                call.pos,
                "`await` takes a call: `await wait(1.0)`, `await self.walk()` or `await load()`",
            ),
        }
    }

    // Ends `cur` with the await and returns the block that resumes after it,
    // which first takes the awaited value with `resume`.
    fn await_call(
        &mut self,
        call: &Expr,
        cur: usize,
        resume: impl FnOnce(Expr) -> Option<StmtKind>,
        pos: Pos,
    ) -> R<usize> {
        let (ty, script) = self.awaited(call)?;
        let site = self.sites;
        self.sites += 1;
        let next = self.new_block();
        match script {
            None => self.seal(
                cur,
                Term::AwaitHost {
                    call: call.clone(),
                    site,
                    next,
                },
            ),
            Some((key, with_self)) => {
                let poll = self.new_block();
                self.seal(
                    cur,
                    Term::AwaitStart {
                        call: call.clone(),
                        site,
                        poll,
                    },
                );
                self.seal(
                    poll,
                    Term::AwaitPoll {
                        key,
                        with_self,
                        site,
                        next,
                    },
                );
            }
        }
        let resumed = Expr {
            kind: ExprKind::Resumed(ty, site),
            pos,
        };
        let kind = resume(resumed.clone()).unwrap_or(StmtKind::Expr(resumed));
        self.blocks[next].stmts.push(stmt(kind, pos));
        Ok(next)
    }

    pub(crate) fn lower(&mut self, stmts: &[Stmt], mut cur: usize, in_loop: bool) -> R<usize> {
        for s in stmts {
            if !awaits(s) && (!in_loop || !escapes(s, 0)) {
                self.blocks[cur].stmts.push(s.clone());
                if matches!(s.kind, StmtKind::Return(_)) {
                    cur = self.new_block();
                }
                continue;
            }
            cur = self.lower_one(s, cur, in_loop)?;
        }
        Ok(cur)
    }

    fn lower_one(&mut self, s: &Stmt, cur: usize, in_loop: bool) -> R<usize> {
        let pos = s.pos;
        let whole = |e: &Expr| match &e.kind {
            ExprKind::Await(call) => Ok((**call).clone()),
            _ => err(e.pos, "`await` must be the whole right side"),
        };
        match &s.kind {
            StmtKind::Let { name, ty, init } => {
                let (name, ty) = (name.clone(), ty.clone());
                self.await_call(
                    &whole(init)?,
                    cur,
                    |r| Some(StmtKind::Let { name, ty, init: r }),
                    pos,
                )
            }
            StmtKind::Assign { target, op, value } => {
                let (target, op) = (target.clone(), *op);
                self.await_call(
                    &whole(value)?,
                    cur,
                    |r| {
                        Some(StmtKind::Assign {
                            target,
                            op,
                            value: r,
                        })
                    },
                    pos,
                )
            }
            StmtKind::Expr(Expr {
                kind: ExprKind::Await(call),
                ..
            }) => self.await_call(call, cur, |_| None, pos),
            StmtKind::Expr(Expr {
                kind: ExprKind::If(cond, then, els),
                ..
            }) => self.lower_if(cond, then, els.as_ref(), cur, in_loop),
            StmtKind::Expr(_) => err(
                pos,
                "`await` cannot sit inside this expression; give it its own statement",
            ),
            StmtKind::Return(_) => err(pos, "write `let v = await ..;` and then `return v;`"),
            StmtKind::Break | StmtKind::Continue => {
                let &(brk, cont) = self.loops.last().ok_or(crate::Error {
                    pos,
                    msg: "`break` outside a loop".into(),
                })?;
                self.seal(
                    cur,
                    Term::Goto(if matches!(s.kind, StmtKind::Break) {
                        brk
                    } else {
                        cont
                    }),
                );
                Ok(self.new_block())
            }
            StmtKind::While { cond, body } => self.lower_loop(Some(cond.clone()), body, cur),
            StmtKind::Loop(body) => self.lower_loop(None, body, cur),
            StmtKind::ForRange {
                var: v,
                from,
                to,
                body,
            } => self.lower_for_range(v, from, to, body, cur, pos),
            StmtKind::ForEach {
                var: v,
                items,
                body,
            } => self.lower_for_each(v, items, body, cur, pos),
        }
    }

    fn let_in(&mut self, block: usize, name: &str, ty: Option<Ty>, init: Expr, pos: Pos) {
        self.blocks[block].stmts.push(stmt(
            StmtKind::Let {
                name: name.to_string(),
                ty,
                init,
            },
            pos,
        ));
    }

    fn lower_for_range(
        &mut self,
        v: &str,
        from: &Expr,
        to: &Expr,
        body: &Block,
        cur: usize,
        pos: Pos,
    ) -> R<usize> {
        self.tmp += 1;
        let end = format!("$end{}", self.tmp);
        self.let_in(cur, v, Some(Ty::Int), from.clone(), pos);
        self.let_in(cur, &end, Some(Ty::Int), to.clone(), pos);
        let cond = Expr {
            kind: ExprKind::Bin(BinOp::Lt, Box::new(var(v, pos)), Box::new(var(&end, pos))),
            pos,
        };
        self.counted(cond, vec![], plus_one(v, pos), body, cur)
    }

    fn lower_for_each(
        &mut self,
        v: &str,
        items: &Expr,
        body: &Block,
        cur: usize,
        pos: Pos,
    ) -> R<usize> {
        self.tmp += 1;
        let (arr, idx) = (format!("$items{}", self.tmp), format!("$i{}", self.tmp));
        self.let_in(cur, &arr, None, items.clone(), pos);
        self.let_in(
            cur,
            &idx,
            Some(Ty::Int),
            Expr {
                kind: ExprKind::Int(0),
                pos,
            },
            pos,
        );
        let len = Expr {
            kind: ExprKind::Method(Box::new(var(&arr, pos)), "len".into(), vec![]),
            pos,
        };
        let cond = Expr {
            kind: ExprKind::Bin(BinOp::Lt, Box::new(var(&idx, pos)), Box::new(len)),
            pos,
        };
        let element = Expr {
            kind: ExprKind::Index(Box::new(var(&arr, pos)), Box::new(var(&idx, pos))),
            pos,
        };
        let take = stmt(
            StmtKind::Let {
                name: v.to_string(),
                ty: None,
                init: element,
            },
            pos,
        );
        self.counted(cond, vec![take], plus_one(&idx, pos), body, cur)
    }

    fn lower_if(
        &mut self,
        cond: &Expr,
        then: &Block,
        els: Option<&Block>,
        cur: usize,
        in_loop: bool,
    ) -> R<usize> {
        let tb = self.new_block();
        let eb = self.new_block();
        let join = self.new_block();
        self.seal(cur, Term::Branch(cond.clone(), tb, eb));
        let end = self.lower(&then.stmts, tb, in_loop)?;
        self.seal(end, Term::Goto(join));
        let end = match els {
            Some(e) => self.lower(&e.stmts, eb, in_loop)?,
            None => eb,
        };
        self.seal(end, Term::Goto(join));
        Ok(join)
    }

    fn lower_loop(&mut self, cond: Option<Expr>, body: &Block, cur: usize) -> R<usize> {
        let head = self.new_block();
        let exit = self.new_block();
        self.seal(cur, Term::Goto(head));
        let first = match cond {
            Some(c) => {
                let b = self.new_block();
                self.seal(head, Term::Branch(c, b, exit));
                b
            }
            None => head,
        };
        self.loops.push((exit, head));
        let end = self.lower(&body.stmts, first, true)?;
        self.loops.pop();
        self.seal(end, Term::Goto(head));
        Ok(exit)
    }

    fn counted(
        &mut self,
        cond: Expr,
        prelude: Vec<Stmt>,
        step: Stmt,
        body: &Block,
        cur: usize,
    ) -> R<usize> {
        let head = self.new_block();
        let bodyb = self.new_block();
        let next = self.new_block();
        let exit = self.new_block();
        self.seal(cur, Term::Goto(head));
        self.seal(head, Term::Branch(cond, bodyb, exit));
        self.blocks[bodyb].stmts.extend(prelude);
        self.blocks[next].stmts.push(step);
        self.seal(next, Term::Goto(head));
        self.loops.push((exit, next));
        let end = self.lower(&body.stmts, bodyb, true)?;
        self.loops.pop();
        self.seal(end, Term::Goto(next));
        Ok(exit)
    }
}
