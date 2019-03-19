use pest::iterators::Pair;
use pest::Parser;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use dhall_parser::{DhallParser, Rule};

use crate::core;
use crate::core::*;

// This file consumes the parse tree generated by pest and turns it into
// our own AST. All those custom macros should eventually moved into
// their own crate because they are quite general and useful. For now they
// are here and hopefully you can figure out how they work.

pub type ParsedExpr = Expr<X, Import>;
pub type ParsedText = InterpolatedText<X, Import>;
pub type ParsedTextContents<'a> = InterpolatedTextContents<'a, X, Import>;
pub type RcExpr = Rc<ParsedExpr>;

pub type ParseError = pest::error::Error<Rule>;

pub type ParseResult<T> = Result<T, ParseError>;

pub fn custom_parse_error(pair: &Pair<Rule>, msg: String) -> ParseError {
    let msg =
        format!("{} while matching on:\n{}", msg, debug_pair(pair.clone()));
    let e = pest::error::ErrorVariant::CustomError { message: msg };
    pest::error::Error::new_from_span(e, pair.as_span())
}

fn debug_pair(pair: Pair<Rule>) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    fn aux(s: &mut String, indent: usize, prefix: String, pair: Pair<Rule>) {
        let indent_str = "| ".repeat(indent);
        let rule = pair.as_rule();
        let contents = pair.as_str();
        let mut inner = pair.into_inner();
        let mut first = true;
        while let Some(p) = inner.next() {
            if first {
                first = false;
                let last = inner.peek().is_none();
                if last && p.as_str() == contents {
                    let prefix = format!("{}{:?} > ", prefix, rule);
                    aux(s, indent, prefix, p);
                    continue;
                } else {
                    writeln!(
                        s,
                        r#"{}{}{:?}: "{}""#,
                        indent_str, prefix, rule, contents
                    )
                    .unwrap();
                }
            }
            aux(s, indent + 1, "".into(), p);
        }
        if first {
            writeln!(
                s,
                r#"{}{}{:?}: "{}""#,
                indent_str, prefix, rule, contents
            )
            .unwrap();
        }
    }
    aux(&mut s, 0, "".into(), pair);
    s
}

#[derive(Debug)]
enum IterMatchError<T> {
    NoMatchFound,
    Other(T), // Allow other macros to inject their own errors
}

/* Extends match_iter with typed matches. Takes a callback that determines
 * when a capture matches.
 * Returns `Result<_, IterMatchError<_>>`; errors returned by the callback will
 * get propagated using IterMatchError::Other.
 *
 * Example:
 * ```
 * macro_rules! callback {
 *     (@type_callback, positive, $x:expr) => {
 *         if $x >= 0 { Ok($x) } else { Err(()) }
 *     };
 *     (@type_callback, negative, $x:expr) => {
 *         if $x <= 0 { Ok($x) } else { Err(()) }
 *     };
 *     (@type_callback, any, $x:expr) => {
 *         Ok($x)
 *     };
 * }
 *
 * let vec = vec![-1, 2, 3];
 *
 * match_iter_typed!(callback; vec.into_iter();
 *     (x: positive, y?: negative, z: any) => { ... },
 * )
 * ```
 *
*/
macro_rules! match_iter_typed {
    // Collect untyped arguments to pass to match_iter!
    (@collect, ($($vars:tt)*), ($($args:tt)*), ($($acc:tt)*), ($x:ident : $ty:ident, $($rest:tt)*)) => {
        match_iter_typed!(@collect, ($($vars)*), ($($args)*), ($($acc)*, $x), ($($rest)*))
    };
    (@collect, ($($vars:tt)*), ($($args:tt)*), ($($acc:tt)*), ($x:ident.. : $ty:ident, $($rest:tt)*)) => {
        match_iter_typed!(@collect, ($($vars)*), ($($args)*), ($($acc)*, $x..), ($($rest)*))
    };
    // Catch extra comma if exists
    (@collect, ($($vars:tt)*), ($($args:tt)*), (,$($acc:tt)*), ($(,)*)) => {
        match_iter_typed!(@collect, ($($vars)*), ($($args)*), ($($acc)*), ())
    };
    (@collect, ($iter:expr, $body:expr, $callback:ident, $error:ident), ($($args:tt)*), ($($acc:tt)*), ($(,)*)) => {
        {
            let res = iter_patterns::destructure_iter!($iter; [$($acc)*] => {
                match_iter_typed!(@callback, $callback, $iter, $($args)*);
                $body
            });
            res.ok_or(IterMatchError::NoMatchFound)
        }
    };

    // Pass the matches through the callback
    (@callback, $callback:ident, $iter:expr, $x:ident : $ty:ident $($rest:tt)*) => {
        let $x = $callback!(@type_callback, $ty, $x);
        #[allow(unused_mut)]
        let mut $x = match $x {
            Ok(x) => x,
            Err(e) => break Err(IterMatchError::Other(e)),
        };
        match_iter_typed!(@callback, $callback, $iter $($rest)*);
    };
    (@callback, $callback: ident, $iter:expr, $x:ident.. : $ty:ident $($rest:tt)*) => {
        let $x = $x.map(|x| $callback!(@type_callback, $ty, x)).collect();
        let $x: Vec<_> = match $x {
            Ok(x) => x,
            Err(e) => break Err(IterMatchError::Other(e)),
        };
        #[allow(unused_mut)]
        let mut $x = $x.into_iter();
        match_iter_typed!(@callback, $callback, $iter $($rest)*);
    };
    (@callback, $callback:ident, $iter:expr $(,)*) => {};

    ($callback:ident; $iter:expr; ($($args:tt)*) => $body:expr) => {
        {
            #[allow(unused_mut)]
            let mut iter = $iter;
            let res: Result<_, IterMatchError<_>> = loop {
                break match_iter_typed!(@collect,
                    (iter, $body, $callback, last_error),
                    ($($args)*), (), ($($args)*,)
                )
            };
            res
        }
    };
}

/* Extends match_iter and match_iter_typed with branching.
 * Returns `Result<_, IterMatchError<_>>`; errors returned by the callback will
 * get propagated using IterMatchError::Other.
 * Allows multiple branches. The passed iterator must be Clone.
 * Will check the branches in order, testing each branch using the callback macro provided.
 *
 * Example:
 * ```
 * macro_rules! callback {
 *     (@type_callback, positive, $x:expr) => {
 *         if $x >= 0 { Ok($x) } else { Err(()) }
 *     };
 *     (@type_callback, negative, $x:expr) => {
 *         if $x <= 0 { Ok($x) } else { Err(()) }
 *     };
 *     (@type_callback, any, $x:expr) => {
 *         Ok($x)
 *     };
 *     (@branch_callback, typed, $($args:tt)*) => {
 *         match_iter_typed!(callback; $($args)*)
 *     };
 *     (@branch_callback, untyped, $($args:tt)*) => {
 *         match_iter!($($args)*)
 *     };
 * }
 *
 * let vec = vec![-1, 2, 3];
 *
 * match_iter_branching!(branch_callback; vec.into_iter();
 *     typed!(x: positive, y?: negative, z: any) => { ... },
 *     untyped!(x, y, z) => { ... },
 * )
 * ```
 *
*/
macro_rules! match_iter_branching {
    (@noclone, $callback:ident; $arg:expr; $( $submac:ident!($($args:tt)*) => $body:expr ),* $(,)*) => {
        {
            #[allow(unused_assignments)]
            let mut last_error = IterMatchError::NoMatchFound;
            // Not a real loop; used for error handling
            // Would use loop labels but they create warnings
            #[allow(unreachable_code)]
            loop {
                $(
                    let matched: Result<_, IterMatchError<_>> =
                        $callback!(@branch_callback, $submac, $arg; ($($args)*) => $body);
                    #[allow(unused_assignments)]
                    match matched {
                        Ok(v) => break Ok(v),
                        Err(e) => last_error = e,
                    };
                )*
                break Err(last_error);
            }
        }
    };
    ($callback:ident; $iter:expr; $($args:tt)*) => {
        {
            #[allow(unused_mut)]
            let mut iter = $iter;
            match_iter_branching!(@noclone, $callback; iter.clone(); $($args)*)
        }
    };
}

macro_rules! match_pair {
    (@type_callback, $ty:ident, $x:expr) => {
        $ty($x)
    };
    (@branch_callback, children, $pair:expr; $($args:tt)*) => {
        {
            #[allow(unused_mut)]
            let mut pairs = $pair.clone().into_inner();
            match_iter_typed!(match_pair; pairs; $($args)*)
        }
    };
    (@branch_callback, self, $pair:expr; ($x:ident : $ty:ident) => $body:expr) => {
        {
            let $x = match_pair!(@type_callback, $ty, $pair.clone());
            match $x {
                Ok($x) => Ok($body),
                Err(e) => Err(IterMatchError::Other(e)),
            }
        }
    };
    (@branch_callback, raw_pair, $pair:expr; ($x:ident) => $body:expr) => {
        {
            let $x = $pair.clone();
            Ok($body)
        }
    };
    (@branch_callback, captured_str, $pair:expr; ($x:ident) => $body:expr) => {
        {
            let $x = $pair.as_str();
            Ok($body)
        }
    };

    ($pair:expr; $($args:tt)*) => {
        {
            let pair = $pair;
            let result = match_iter_branching!(@noclone, match_pair; pair; $($args)*);
            result.map_err(|e| match e {
                IterMatchError::Other(e) => e,
                _ => custom_parse_error(&pair, "No match found".to_owned()),
            })
        }
    };
}

macro_rules! make_pest_parse_function {
    ($name:ident<$o:ty>; $submac:ident!( $($args:tt)* )) => (
        #[allow(unused_variables)]
        #[allow(non_snake_case)]
        #[allow(clippy::all)]
        fn $name<'a>(pair: Pair<'a, Rule>) -> ParseResult<$o> {
            $submac!(pair; $($args)*)
        }
    );
}

macro_rules! named {
    ($name:ident<$o:ty>; $($args:tt)*) => (
        make_pest_parse_function!($name<$o>; match_pair!( $($args)* ));
    );
}

macro_rules! rule {
    ($name:ident<$o:ty>; $($args:tt)*) => (
        make_pest_parse_function!($name<$o>; match_rule!(
            Rule::$name => match_pair!( $($args)* ),
        ));
    );
}

macro_rules! rule_group {
    ($name:ident<$o:ty>; $($ty:ident),*) => (
        make_pest_parse_function!($name<$o>; match_rule!(
            $(
                Rule::$ty => match_pair!(raw_pair!(p) => $ty(p)?),
            )*
        ));
    );
}

macro_rules! match_rule {
    ($pair:expr; $($pat:pat => $submac:ident!( $($args:tt)* ),)*) => {
        {
            #[allow(unreachable_patterns)]
            match $pair.as_rule() {
                $(
                    $pat => $submac!($pair; $($args)*),
                )*
                r => Err(custom_parse_error(&$pair, format!("Unexpected {:?}", r))),
            }
        }
    };
}

rule!(EOI<()>; children!() => ());

named!(str<&'a str>; captured_str!(s) => s.trim());

named!(raw_str<&'a str>; captured_str!(s) => s);

named!(label<Label>; captured_str!(s) => Label::from(s.trim().to_owned()));

rule!(double_quote_literal<ParsedText>;
    children!(chunks..: double_quote_chunk) => {
        chunks.collect()
    }
);

rule!(double_quote_chunk<ParsedTextContents<'a>>;
    children!(c: interpolation) => {
        InterpolatedTextContents::Expr(c)
    },
    children!(s: double_quote_escaped) => {
        InterpolatedTextContents::Text(s)
    },
    captured_str!(s) => {
        InterpolatedTextContents::Text(s)
    },
);
rule!(double_quote_escaped<&'a str>;
    // TODO: parse all escapes
    captured_str!(s) => {
        match s {
            "\"" => "\"",
            "$" => "$",
            "\\" => "\\",
            "/" => "/",
            // "b" => "\b",
            // "f" => "\f",
            "n" => "\n",
            "r" => "\r",
            "t" => "\t",
            // "uXXXX"
            _ => unimplemented!(),
        }
    }
);

rule!(single_quote_literal<ParsedText>;
    children!(eol: raw_str, contents: single_quote_continue) => {
        contents.into_iter().rev().collect::<ParsedText>()
    }
);
rule!(escaped_quote_pair<&'a str>;
    children!() => "''"
);
rule!(escaped_interpolation<&'a str>;
    children!() => "${"
);
rule!(interpolation<RcExpr>;
    children!(e: expression) => e
);

rule!(single_quote_continue<Vec<ParsedTextContents<'a>>>;
    children!(c: interpolation, rest: single_quote_continue) => {
        rest.push(InterpolatedTextContents::Expr(c)); rest
    },
    children!(c: escaped_quote_pair, rest: single_quote_continue) => {
        rest.push(InterpolatedTextContents::Text(c)); rest
    },
    children!(c: escaped_interpolation, rest: single_quote_continue) => {
        rest.push(InterpolatedTextContents::Text(c)); rest
    },
    children!(c: raw_str, rest: single_quote_continue) => {
        rest.push(InterpolatedTextContents::Text(c)); rest
    },
    children!() => {
        vec![]
    },
);

rule!(NaN_raw<()>; children!() => ());
rule!(minus_infinity_literal<()>; children!() => ());
rule!(plus_infinity_literal<()>; children!() => ());

rule!(double_literal_raw<core::Double>;
    raw_pair!(pair) => {
        pair.as_str().trim()
            .parse()
            .map_err(|e: std::num::ParseFloatError| custom_parse_error(&pair, format!("{}", e)))?
    }
);

rule!(natural_literal_raw<core::Natural>;
    raw_pair!(pair) => {
        pair.as_str().trim()
            .parse()
            .map_err(|e: std::num::ParseIntError| custom_parse_error(&pair, format!("{}", e)))?
    }
);

rule!(integer_literal_raw<core::Integer>;
    raw_pair!(pair) => {
        pair.as_str().trim()
            .parse()
            .map_err(|e: std::num::ParseIntError| custom_parse_error(&pair, format!("{}", e)))?
    }
);

rule!(path<PathBuf>;
    captured_str!(s) => (".".to_owned() + s).into()
);

rule!(parent_path<(FilePrefix, PathBuf)>;
    children!(p: path) => (FilePrefix::Parent, p)
);

rule!(here_path<(FilePrefix, PathBuf)>;
    children!(p: path) => (FilePrefix::Here, p)
);

rule!(home_path<(FilePrefix, PathBuf)>;
    children!(p: path) => (FilePrefix::Home, p)
);

rule!(absolute_path<(FilePrefix, PathBuf)>;
    children!(p: path) => (FilePrefix::Absolute, p)
);

rule_group!(local_raw<(FilePrefix, PathBuf)>;
    parent_path,
    here_path,
    home_path,
    absolute_path
);

// TODO: other import types
rule!(import_type_raw<ImportLocation>;
    // children!(_e: missing_raw) => {
    //     ImportLocation::Missing
    // }
    // children!(e: env_raw) => {
    //     ImportLocation::Env(e)
    // }
    // children!(url: http) => {
    //     ImportLocation::Remote(url)
    // }
    children!(import: local_raw) => {
        let (prefix, path) = import;
        ImportLocation::Local(prefix, path)
    }
);

rule!(import_hashed_raw<(ImportLocation, Option<()>)>;
    // TODO: handle hash
    children!(import: import_type_raw) => {
        (import, None)
    }
);

rule!(import_raw<RcExpr>;
    // TODO: handle "as Text"
    children!(import: import_hashed_raw) => {
        let (location, hash) = import;
        bx(Expr::Embed(Import {
            mode: ImportMode::Code,
            hash,
            location,
        }))
    }
);

rule_group!(expression<RcExpr>;
    identifier_raw,
    lambda_expression,
    ifthenelse_expression,
    let_expression,
    forall_expression,
    arrow_expression,
    merge_expression,
    empty_collection,
    non_empty_optional,

    annotated_expression,
    import_alt_expression,
    or_expression,
    plus_expression,
    text_append_expression,
    list_append_expression,
    and_expression,
    combine_expression,
    prefer_expression,
    combine_types_expression,
    times_expression,
    equal_expression,
    not_equal_expression,
    application_expression,

    import_raw,
    selector_expression_raw,
    literal_expression_raw,
    empty_record_type,
    empty_record_literal,
    non_empty_record_type_or_literal,
    union_type_or_literal,
    non_empty_list_literal_raw,
    final_expression
);

rule!(lambda_expression<RcExpr>;
    children!(l: label, typ: expression, body: expression) => {
        bx(Expr::Lam(l, typ, body))
    }
);

rule!(ifthenelse_expression<RcExpr>;
    children!(cond: expression, left: expression, right: expression) => {
        bx(Expr::BoolIf(cond, left, right))
    }
);

rule!(let_expression<RcExpr>;
    children!(bindings..: let_binding, final_expr: expression) => {
        bindings.fold(final_expr, |acc, x| bx(Expr::Let(x.0, x.1, x.2, acc)))
    }
);

rule!(let_binding<(Label, Option<RcExpr>, RcExpr)>;
    children!(name: label, annot: expression, expr: expression) => (name, Some(annot), expr),
    children!(name: label, expr: expression) => (name, None, expr),
);

rule!(forall_expression<RcExpr>;
    children!(l: label, typ: expression, body: expression) => {
        bx(Expr::Pi(l, typ, body))
    }
);

rule!(arrow_expression<RcExpr>;
    children!(typ: expression, body: expression) => {
        bx(Expr::Pi("_".into(), typ, body))
    }
);

rule!(merge_expression<RcExpr>;
    children!(x: expression, y: expression, z: expression) => bx(Expr::Merge(x, y, Some(z))),
    children!(x: expression, y: expression) => bx(Expr::Merge(x, y, None)),
);

rule!(empty_collection<RcExpr>;
    children!(x: str, y: expression) => {
       match x {
          "Optional" => bx(Expr::OptionalLit(Some(y), None)),
          "List" => bx(Expr::EmptyListLit(y)),
          _ => unreachable!(),
       }
    }
);

rule!(non_empty_optional<RcExpr>;
    children!(x: expression, _y: str, z: expression) => {
        bx(Expr::OptionalLit(Some(z), Some(x)))
    }
);

// List of rules that can be shortcutted as implemented in binop!()
fn can_be_shortcutted(rule: Rule) -> bool {
    use Rule::*;
    match rule {
        import_alt_expression
        | or_expression
        | plus_expression
        | text_append_expression
        | list_append_expression
        | and_expression
        | combine_expression
        | prefer_expression
        | combine_types_expression
        | times_expression
        | equal_expression
        | not_equal_expression
        | application_expression
        | selector_expression_raw
        | annotated_expression => true,
        _ => false,
    }
}

macro_rules! binop {
    ($rule:ident, $op:ident) => {
        rule!($rule<RcExpr>;
            raw_pair!(pair) => {
                // This all could be a trivial fold, but to avoid stack explosion
                // we try to cut down on the recursion level here, by consuming
                // chains of blah_expression > ... > blih_expression in one go.
                let mut pair = pair;
                let mut pairs = pair.into_inner();
                let first = pairs.next().unwrap();
                let rest: Vec<_> = pairs.map(expression).collect::<Result<_, _>>()?;
                if !rest.is_empty() {
                    // If there is more than one subexpression, handle it normally
                    let first = expression(first)?;
                    rest.into_iter().fold(first, |acc, e| bx(Expr::BinOp(BinOp::$op, acc, e)))
                } else {
                    // Otherwise, consume short-cuttable rules as long as they contain only one subexpression.
                    // println!("short-cutting {}", debug_pair(pair.clone()));
                    pair = first;
                    while can_be_shortcutted(pair.as_rule()) {
                        let mut pairs = pair.clone().into_inner();
                        let first = pairs.next().unwrap();
                        let rest: Vec<_> = pairs.collect();
                        if !rest.is_empty() {
                            break;
                        }
                        pair = first;
                    }
                    // println!("short-cutted {}", debug_pair(pair.clone()));
                    // println!();
                    expression(pair)?
                }
            }
            // children!(first: expression, rest..: expression) => {
            //     rest.fold(first, |acc, e| bx(Expr::BinOp(BinOp::$op, acc, e)))
            // }
        );
    };
}

binop!(import_alt_expression, ImportAlt);
binop!(or_expression, BoolOr);
binop!(plus_expression, NaturalPlus);
binop!(text_append_expression, TextAppend);
binop!(list_append_expression, ListAppend);
binop!(and_expression, BoolAnd);
binop!(combine_expression, Combine);
binop!(prefer_expression, Prefer);
binop!(combine_types_expression, CombineTypes);
binop!(times_expression, NaturalTimes);
binop!(equal_expression, BoolEQ);
binop!(not_equal_expression, BoolNE);

rule!(annotated_expression<RcExpr>;
    children!(e: expression, annot: expression) => {
        bx(Expr::Annot(e, annot))
    },
    children!(e: expression) => e,
);

rule!(application_expression<RcExpr>;
    children!(first: expression, rest..: expression) => {
        let rest: Vec<_> = rest.collect();
        if rest.is_empty() {
            first
        } else {
            bx(Expr::App(first, rest))
        }
    }
);

rule!(selector_expression_raw<RcExpr>;
    children!(first: expression, rest..: label) => {
        rest.fold(first, |acc, e| bx(Expr::Field(acc, e)))
    }
);

rule!(literal_expression_raw<RcExpr>;
    children!(n: double_literal_raw) => bx(Expr::DoubleLit(n)),
    children!(n: minus_infinity_literal) => bx(Expr::DoubleLit(std::f64::NEG_INFINITY)),
    children!(n: plus_infinity_literal) => bx(Expr::DoubleLit(std::f64::INFINITY)),
    children!(n: NaN_raw) => bx(Expr::DoubleLit(std::f64::NAN)),
    children!(n: natural_literal_raw) => bx(Expr::NaturalLit(n)),
    children!(n: integer_literal_raw) => bx(Expr::IntegerLit(n)),
    children!(s: double_quote_literal) => bx(Expr::TextLit(s)),
    children!(s: single_quote_literal) => bx(Expr::TextLit(s)),
    children!(e: expression) => e,
);

rule!(identifier_raw<RcExpr>;
    children!(name: str, idx: natural_literal_raw) => {
        match Builtin::parse(name) {
            Some(b) => bx(Expr::Builtin(b)),
            None => match name {
                "True" => bx(Expr::BoolLit(true)),
                "False" => bx(Expr::BoolLit(false)),
                "Type" => bx(Expr::Const(Const::Type)),
                "Kind" => bx(Expr::Const(Const::Kind)),
                name => bx(Expr::Var(V(Label::from(name.to_owned()), idx))),
            }
        }
    },
    children!(name: str) => {
        match Builtin::parse(name) {
            Some(b) => bx(Expr::Builtin(b)),
            None => match name {
                "True" => bx(Expr::BoolLit(true)),
                "False" => bx(Expr::BoolLit(false)),
                "Type" => bx(Expr::Const(Const::Type)),
                "Kind" => bx(Expr::Const(Const::Kind)),
                name => bx(Expr::Var(V(Label::from(name.to_owned()), 0))),
            }
        }
    },
);

rule!(empty_record_literal<RcExpr>;
    children!() => bx(Expr::RecordLit(BTreeMap::new()))
);

rule!(empty_record_type<RcExpr>;
    children!() => bx(Expr::Record(BTreeMap::new()))
);

rule!(non_empty_record_type_or_literal<RcExpr>;
    children!(first_label: label, rest: non_empty_record_type) => {
        let (first_expr, mut map) = rest;
        map.insert(first_label, first_expr);
        bx(Expr::Record(map))
    },
    children!(first_label: label, rest: non_empty_record_literal) => {
        let (first_expr, mut map) = rest;
        map.insert(first_label, first_expr);
        bx(Expr::RecordLit(map))
    },
);

rule!(non_empty_record_type<(RcExpr, BTreeMap<Label, RcExpr>)>;
    self!(x: partial_record_entries) => x
);

named!(partial_record_entries<(RcExpr, BTreeMap<Label, RcExpr>)>;
    children!(expr: expression, entries..: record_entry) => {
        (expr, entries.collect())
    }
);

named!(record_entry<(Label, RcExpr)>;
    children!(name: label, expr: expression) => (name, expr)
);

rule!(non_empty_record_literal<(RcExpr, BTreeMap<Label, RcExpr>)>;
    self!(x: partial_record_entries) => x
);

rule!(union_type_or_literal<RcExpr>;
    children!(_e: empty_union_type) => {
        bx(Expr::Union(BTreeMap::new()))
    },
    children!(x: non_empty_union_type_or_literal) => {
        match x {
            (Some((l, e)), entries) => bx(Expr::UnionLit(l, e, entries)),
            (None, entries) => bx(Expr::Union(entries)),
        }
    },
);

rule!(empty_union_type<()>; children!() => ());

rule!(non_empty_union_type_or_literal
      <(Option<(Label, RcExpr)>, BTreeMap<Label, RcExpr>)>;
    children!(l: label, e: expression, entries: union_type_entries) => {
        (Some((l, e)), entries)
    },
    children!(l: label, e: expression, rest: non_empty_union_type_or_literal) => {
        let (x, mut entries) = rest;
        entries.insert(l, e);
        (x, entries)
    },
    children!(l: label, e: expression) => {
        let mut entries = BTreeMap::new();
        entries.insert(l, e);
        (None, entries)
    },
);

rule!(union_type_entries<BTreeMap<Label, RcExpr>>;
    children!(entries..: union_type_entry) => {
        entries.collect()
    }
);

rule!(union_type_entry<(Label, RcExpr)>;
    children!(name: label, expr: expression) => (name, expr)
);

rule!(non_empty_list_literal_raw<RcExpr>;
    children!(items..: expression) => {
        bx(Expr::NEListLit(items.collect()))
    }
);

rule!(final_expression<RcExpr>;
    children!(e: expression, _eoi: EOI) => e
);

pub fn parse_expr(s: &str) -> ParseResult<RcExpr> {
    let pairs = DhallParser::parse(Rule::final_expression, s)?;
    // Match the only item in the pairs iterator
    // println!("{}", debug_pair(pairs.clone().next().unwrap()));
    iter_patterns::destructure_iter!(pairs; [p] => expression(p)).unwrap()
    // Ok(bx(Expr::BoolLit(false)))
}

#[test]
fn test_parse() {
    // let expr = r#"{ x = "foo", y = 4 }.x"#;
    // let expr = r#"(1 + 2) * 3"#;
    let expr = r#"(1) + 3 * 5"#;
    println!("{:?}", parse_expr(expr));
    match parse_expr(expr) {
        Err(e) => {
            println!("{:?}", e);
            println!("{}", e);
        }
        ok => println!("{:?}", ok),
    };
    // assert!(false);
}
