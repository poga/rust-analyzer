use std::iter::successors;

use hir::db::HirDatabase;
use ra_syntax::{
    ast::{self, AstNode},
    Direction, TextUnit,
};

use crate::{Assist, AssistCtx, AssistId, TextRange};

// Assist: merge_match_arms
//
// Merges identical match arms.
//
// ```
// enum Action { Move { distance: u32 }, Stop }
//
// fn handle(action: Action) {
//     match action {
//         <|>Action::Move(..) => foo(),
//         Action::Stop => foo(),
//     }
// }
// ```
// ->
// ```
// enum Action { Move { distance: u32 }, Stop }
//
// fn handle(action: Action) {
//     match action {
//         Action::Move(..) | Action::Stop => foo(),
//     }
// }
// ```
pub(crate) fn merge_match_arms(ctx: AssistCtx<impl HirDatabase>) -> Option<Assist> {
    let current_arm = ctx.find_node_at_offset::<ast::MatchArm>()?;
    // Don't try to handle arms with guards for now - can add support for this later
    if current_arm.guard().is_some() {
        return None;
    }
    let current_expr = current_arm.expr()?;
    let current_text_range = current_arm.syntax().text_range();

    enum CursorPos {
        InExpr(TextUnit),
        InPat(TextUnit),
    }
    let cursor_pos = ctx.frange.range.start();
    let cursor_pos = if current_expr.syntax().text_range().contains(cursor_pos) {
        CursorPos::InExpr(current_text_range.end() - cursor_pos)
    } else {
        CursorPos::InPat(cursor_pos)
    };

    // We check if the following match arms match this one. We could, but don't,
    // compare to the previous match arm as well.
    let arms_to_merge = successors(Some(current_arm), next_arm)
        .take_while(|arm| {
            if arm.guard().is_some() {
                return false;
            }
            match arm.expr() {
                Some(expr) => expr.syntax().text() == current_expr.syntax().text(),
                None => false,
            }
        })
        .collect::<Vec<_>>();

    if arms_to_merge.len() <= 1 {
        return None;
    }

    ctx.add_assist(AssistId("merge_match_arms"), "Merge match arms", |edit| {
        let pats = if arms_to_merge.iter().any(contains_placeholder) {
            "_".into()
        } else {
            arms_to_merge
                .iter()
                .flat_map(ast::MatchArm::pats)
                .map(|x| x.syntax().to_string())
                .collect::<Vec<String>>()
                .join(" | ")
        };

        let arm = format!("{} => {}", pats, current_expr.syntax().text());

        let start = arms_to_merge.first().unwrap().syntax().text_range().start();
        let end = arms_to_merge.last().unwrap().syntax().text_range().end();

        edit.target(current_text_range);
        edit.set_cursor(match cursor_pos {
            CursorPos::InExpr(back_offset) => start + TextUnit::from_usize(arm.len()) - back_offset,
            CursorPos::InPat(offset) => offset,
        });
        edit.replace(TextRange::from_to(start, end), arm);
    })
}

fn contains_placeholder(a: &ast::MatchArm) -> bool {
    a.pats().any(|x| match x {
        ra_syntax::ast::Pat::PlaceholderPat(..) => true,
        _ => false,
    })
}

fn next_arm(arm: &ast::MatchArm) -> Option<ast::MatchArm> {
    arm.syntax().siblings(Direction::Next).skip(1).find_map(ast::MatchArm::cast)
}

#[cfg(test)]
mod tests {
    use super::merge_match_arms;
    use crate::helpers::{check_assist, check_assist_not_applicable};

    #[test]
    fn merge_match_arms_single_patterns() {
        check_assist(
            merge_match_arms,
            r#"
            #[derive(Debug)]
            enum X { A, B, C }

            fn main() {
                let x = X::A;
                let y = match x {
                    X::A => { 1i32<|> }
                    X::B => { 1i32 }
                    X::C => { 2i32 }
                }
            }
            "#,
            r#"
            #[derive(Debug)]
            enum X { A, B, C }

            fn main() {
                let x = X::A;
                let y = match x {
                    X::A | X::B => { 1i32<|> }
                    X::C => { 2i32 }
                }
            }
            "#,
        );
    }

    #[test]
    fn merge_match_arms_multiple_patterns() {
        check_assist(
            merge_match_arms,
            r#"
            #[derive(Debug)]
            enum X { A, B, C, D, E }

            fn main() {
                let x = X::A;
                let y = match x {
                    X::A | X::B => {<|> 1i32 },
                    X::C | X::D => { 1i32 },
                    X::E => { 2i32 },
                }
            }
            "#,
            r#"
            #[derive(Debug)]
            enum X { A, B, C, D, E }

            fn main() {
                let x = X::A;
                let y = match x {
                    X::A | X::B | X::C | X::D => {<|> 1i32 },
                    X::E => { 2i32 },
                }
            }
            "#,
        );
    }

    #[test]
    fn merge_match_arms_placeholder_pattern() {
        check_assist(
            merge_match_arms,
            r#"
            #[derive(Debug)]
            enum X { A, B, C, D, E }

            fn main() {
                let x = X::A;
                let y = match x {
                    X::A => { 1i32 },
                    X::B => { 2i<|>32 },
                    _ => { 2i32 }
                }
            }
            "#,
            r#"
            #[derive(Debug)]
            enum X { A, B, C, D, E }

            fn main() {
                let x = X::A;
                let y = match x {
                    X::A => { 1i32 },
                    _ => { 2i<|>32 }
                }
            }
            "#,
        );
    }

    #[test]
    fn merges_all_subsequent_arms() {
        check_assist(
            merge_match_arms,
            r#"
            enum X { A, B, C, D, E }

            fn main() {
                match X::A {
                    X::A<|> => 92,
                    X::B => 92,
                    X::C => 92,
                    X::D => 62,
                    _ => panic!(),
                }
            }
            "#,
            r#"
            enum X { A, B, C, D, E }

            fn main() {
                match X::A {
                    X::A<|> | X::B | X::C => 92,
                    X::D => 62,
                    _ => panic!(),
                }
            }
            "#,
        )
    }

    #[test]
    fn merge_match_arms_rejects_guards() {
        check_assist_not_applicable(
            merge_match_arms,
            r#"
            #[derive(Debug)]
            enum X {
                A(i32),
                B,
                C
            }

            fn main() {
                let x = X::A;
                let y = match x {
                    X::A(a) if a > 5 => { <|>1i32 },
                    X::B => { 1i32 },
                    X::C => { 2i32 }
                }
            }
            "#,
        );
    }
}
