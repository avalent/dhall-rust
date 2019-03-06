#![allow(non_snake_case)]
use std::fmt;
use dhall_core::core::*;

/// Reduce an expression to its normal form, performing beta reduction
///
/// `normalize` does not type-check the expression.  You may want to type-check
/// expressions before normalizing them since normalization can convert an
/// ill-typed expression into a well-typed expression.
///
/// However, `normalize` will not fail if the expression is ill-typed and will
/// leave ill-typed sub-expressions unevaluated.
///
pub fn normalize<'i, S, T, A>(e: &Expr<'i, S, A>) -> Expr<'i, T, A>
where
    S: Clone + fmt::Debug,
    T: Clone + fmt::Debug,
    A: Clone + fmt::Debug,
{
    use dhall_core::BinOp::*;
    use dhall_core::Builtin::*;
    use dhall_core::Expr::*;
    match *e {
        Const(k) => Const(k),
        Var(v) => Var(v),
        Lam(x, ref tA, ref b) => {
            let tA2 = normalize(tA);
            let b2 = normalize(b);
            Lam(x, bx(tA2), bx(b2))
        }
        Pi(x, ref tA, ref tB) => {
            let tA2 = normalize(tA);
            let tB2 = normalize(tB);
            pi(x, tA2, tB2)
        }
        App(ref f, ref a) => match normalize::<S, T, A>(f) {
            Lam(x, _A, b) => {
                // Beta reduce
                let vx0 = V(x, 0);
                let a2 = shift::<S, S, A>(1, vx0, a);
                let b2 = subst::<S, T, A>(vx0, &a2, &b);
                let b3 = shift::<S, T, A>(-1, vx0, &b2);
                normalize(&b3)
            }
            f2 => match (f2, normalize::<S, T, A>(a)) {
                // fold/build fusion for `List`
                (App(box Builtin(ListBuild), _), App(box App(box Builtin(ListFold), _), box e2)) |
                (App(box Builtin(ListFold), _), App(box App(box Builtin(ListBuild), _), box e2)) |

                // fold/build fusion for `Natural`
                (Builtin(NaturalBuild), App(box Builtin(NaturalFold), box e2)) |
                (Builtin(NaturalFold), App(box Builtin(NaturalBuild), box e2)) => normalize(&e2),

            /*
                App (App (App (App NaturalFold (NaturalLit n0)) _) succ') zero ->
                    normalize (go n0)
                  where
                    go !0 = zero
                    go !n = App succ' (go (n - 1))
                App NaturalBuild k
                    | check     -> NaturalLit n
                    | otherwise -> App f' a'
                  where
                    labeled =
                        normalize (App (App (App k Natural) "Succ") "Zero")

                    n = go 0 labeled
                      where
                        go !m (App (Var "Succ") e') = go (m + 1) e'
                        go !m (Var "Zero")          = m
                        go !_  _                    = internalError text
                    check = go labeled
                      where
                        go (App (Var "Succ") e') = go e'
                        go (Var "Zero")          = True
                        go  _                    = False
                        */
                (Builtin(NaturalIsZero), NaturalLit(n)) => BoolLit(n == 0),
                (Builtin(NaturalEven), NaturalLit(n)) => BoolLit(n % 2 == 0),
                (Builtin(NaturalOdd), NaturalLit(n)) => BoolLit(n % 2 != 0),
                (Builtin(NaturalToInteger), NaturalLit(n)) => IntegerLit(n as isize),
                (Builtin(NaturalShow), NaturalLit(n)) => TextLit(n.to_string()),
                (App(f@box Builtin(ListBuild), box t), k) => {
                        let labeled =
                            normalize::<_, T, _>(&app(app(app(k.clone(), app(
                                Builtin(self::Builtin::List), t.clone())), "Cons"), "Nil"));

                        fn list_to_vector<'i, S, A>(v: &mut Vec<Expr<'i, S, A>>, e: Expr<'i, S, A>)
                            where S: Clone, A: Clone
                        {
                            match e {
                                App(box App(box Var(V("Cons", _)), box x), box e2) => {
                                    v.push(x);
                                    list_to_vector(v, e2)
                                }
                                Var(V("Nil", _)) => {}
                                _ => panic!("internalError list_to_vector"),
                            }
                        }
                        fn check<S, A>(e: &Expr<S, A>) -> bool {
                            match *e {
                                App(box App(box Var(V("Cons", _)), _), ref e2) => check(e2),
                                Var(V("Nil", _)) => true,
                                _ => false,
                            }
                        }

                        if check(&labeled) {
                            let mut v = vec![];
                            list_to_vector(&mut v, labeled);
                            ListLit(Some(bx(t)), v)
                        } else {
                            app(App(f, bx(t)), k)
                        }
                    }
                (App(box App(box App(box App(box Builtin(ListFold), _), box ListLit(_, xs)), _), cons), nil) => {
                    let e2: Expr<_, _> = xs.into_iter().rev().fold(nil, |y, ys| // foldr
                        App(bx(App(cons.clone(), bx(y))), bx(ys))
                    );
                    normalize(&e2)
                }
                (App(f, x_), ListLit(t, ys)) => match *f {
                    Builtin(ListLength) =>
                        NaturalLit(ys.len()),
                    Builtin(ListHead) =>
                        normalize(&OptionalLit(t, ys.into_iter().take(1).collect())),
                    Builtin(ListLast) =>
                        normalize(&OptionalLit(t, ys.into_iter().last().into_iter().collect())),
                    Builtin(ListReverse) => {
                        let mut xs = ys;
                        xs.reverse();
                        normalize(&ListLit(t, xs))
                    }
                    _ => app(App(f, x_), ListLit(t, ys)),
                },
                /*
                App (App ListIndexed _) (ListLit t xs) ->
                    normalize (ListLit t' (fmap adapt (Data.Vector.indexed xs)))
                  where
                    t' = Record (Data.Map.fromList kts)
                      where
                        kts = [ ("index", Natural)
                              , ("value", t)
                              ]
                    adapt (n, x) = RecordLit (Data.Map.fromList kvs)
                      where
                        kvs = [ ("index", NaturalLit (fromIntegral n))
                              , ("value", x)
                              ]
            */
                (App(box App(box App(box App(box Builtin(OptionalFold), _), box OptionalLit(_, xs)), _), just), nothing) => {
                    let e2: Expr<_, _> = xs.into_iter().fold(nothing, |y, _|
                        App(just.clone(), bx(y))
                    );
                    normalize(&e2)
                }
                (App(box Builtin(OptionalBuild), _), App(box App(box Builtin(OptionalFold), _), b)) => {
                    normalize(&b)
                }
                (App(box Builtin(OptionalBuild), a0), g) => {
                    let e2: Expr<_, _> = app(app(app(g,
                        App(bx(Builtin(Optional)), a0.clone())),
                            Lam("x", a0.clone(),
                                bx(OptionalLit(Some(a0.clone()), vec![Var(V("x", 0))])))),
                            OptionalLit(Some(a0), vec![]));
                    normalize(&e2)
                }
                (f2, a2) => app(f2, a2),
            },
        },
        Let(f, _, ref r, ref b) => {
            let r2 = shift::<_, S, _>(1, V(f, 0), r);
            let b2 = subst(V(f, 0), &r2, b);
            let b3 = shift::<_, T, _>(-1, V(f, 0), &b2);
            normalize(&b3)
        }
        Annot(ref x, _) => normalize(x),
        Builtin(v) => Builtin(v),
        BoolLit(b) => BoolLit(b),
        BinOp(BoolAnd, ref x, ref y) => with_binop(
            BoolAnd,
            Expr::bool_lit,
            |xn, yn| BoolLit(xn && yn),
            normalize(x),
            normalize(y),
        ),
        BinOp(BoolOr, ref x, ref y) => with_binop(
            BoolOr,
            Expr::bool_lit,
            |xn, yn| BoolLit(xn || yn),
            normalize(x),
            normalize(y),
        ),
        BinOp(BoolEQ, ref x, ref y) => with_binop(
            BoolEQ,
            Expr::bool_lit,
            |xn, yn| BoolLit(xn == yn),
            normalize(x),
            normalize(y),
        ),
        BinOp(BoolNE, ref x, ref y) => with_binop(
            BoolNE,
            Expr::bool_lit,
            |xn, yn| BoolLit(xn != yn),
            normalize(x),
            normalize(y),
        ),
        BoolIf(ref b, ref t, ref f) => match normalize(b) {
            BoolLit(true) => normalize(t),
            BoolLit(false) => normalize(f),
            b2 => BoolIf(bx(b2), bx(normalize(t)), bx(normalize(f))),
        },
        NaturalLit(n) => NaturalLit(n),
        BinOp(NaturalPlus, ref x, ref y) => with_binop(
            NaturalPlus,
            Expr::natural_lit,
            |xn, yn| NaturalLit(xn + yn),
            normalize(x),
            normalize(y),
        ),
        BinOp(NaturalTimes, ref x, ref y) => with_binop(
            NaturalTimes,
            Expr::natural_lit,
            |xn, yn| NaturalLit(xn * yn),
            normalize(x),
            normalize(y),
        ),
        IntegerLit(n) => IntegerLit(n),
        DoubleLit(n) => DoubleLit(n),
        TextLit(ref t) => TextLit(t.clone()),
        BinOp(TextAppend, ref x, ref y) => with_binop(
            TextAppend,
            Expr::text_lit,
            |xt, yt| TextLit(xt + &yt),
            normalize(x),
            normalize(y),
        ),
        ListLit(ref t, ref es) => {
            let t2 = t.as_ref().map(|x| x.as_ref()).map(normalize).map(bx);
            let es2 = es.iter().map(normalize).collect();
            ListLit(t2, es2)
        }
        OptionalLit(ref t, ref es) => {
            let t2 = t.as_ref().map(|x| x.as_ref()).map(normalize).map(bx);
            let es2 = es.iter().map(normalize).collect();
            OptionalLit(t2, es2)
        }
        Record(ref kts) => Record(map_record_value(kts, normalize)),
        RecordLit(ref kvs) => RecordLit(map_record_value(kvs, normalize)),
        Union(ref kts) => Union(map_record_value(kts, normalize)),
        UnionLit(k, ref v, ref kvs) => {
            UnionLit(k, bx(normalize(v)), map_record_value(kvs, normalize))
        }
        Merge(ref _x, ref _y, ref _t) => unimplemented!(),
        Field(ref r, x) => match normalize(r) {
            RecordLit(kvs) => match kvs.get(x) {
                Some(r2) => normalize(r2),
                None => {
                    Field(bx(RecordLit(map_record_value(&kvs, normalize))), x)
                }
            },
            r2 => Field(bx(r2), x),
        },
        Note(_, ref e) => normalize(e),
        Embed(ref a) => Embed(a.clone()),
        _ => unimplemented!(),
    }
}

fn with_binop<'a, S, A, U, Get, Set>(
    op: BinOp,
    get: Get,
    set: Set,
    x: Expr<'a, S, A>,
    y: Expr<'a, S, A>,
) -> Expr<'a, S, A>
where
    Get: Fn(&Expr<'a, S, A>) -> Option<U>,
    Set: FnOnce(U, U) -> Expr<'a, S, A>,
{
    if let (Some(xv), Some(yv)) = (get(&x), get(&y)) {
        set(xv, yv)
    } else {
        Expr::BinOp(op, bx(x), bx(y))
    }
}
